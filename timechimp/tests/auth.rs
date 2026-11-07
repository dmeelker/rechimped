//! The Cognito flows (refresh, retry after a 401, SRP login with MFA) against a local server that
//! stands in for both Cognito and the API.

use timechimp::{Authenticator, Client, LoginOutcome, MfaKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Answers one request per entry of `responses` (status, JSON body), in order; returns the base
/// URL and the raw requests (headers lowercased, body as sent).
async fn serve(responses: Vec<(&'static str, String)>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/", listener.local_addr().unwrap());
    let requests = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            let header_end = loop {
                if let Some(i) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i + 4;
                }
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "connection closed before the headers were complete");
                request.extend_from_slice(&buf[..n]);
            };
            let headers = String::from_utf8(request[..header_end].to_vec()).unwrap().to_lowercase();
            let length = headers
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .map_or(0, |n| n.trim().parse::<usize>().unwrap());
            while request.len() < header_end + length {
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "connection closed before the body was complete");
                request.extend_from_slice(&buf[..n]);
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            requests.push(headers + std::str::from_utf8(&request[header_end..]).unwrap());
        }
        requests
    });
    (base, requests)
}

fn tokens(access: &str) -> (&'static str, String) {
    ("200 OK", format!(r#"{{"AuthenticationResult": {{"AccessToken": "{access}", "ExpiresIn": 3600}}}}"#))
}

fn current_user() -> (&'static str, String) {
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/current_user.json")).unwrap();
    ("200 OK", fixture)
}

#[tokio::test]
async fn refreshes_the_access_token_first() {
    let (base, requests) = serve(vec![tokens("t1"), current_user()]).await;
    let client = Client::with_base_url(Authenticator::from_refresh_token_at(&base, "r1"), base);
    client.current_user().await.unwrap();

    let requests = requests.await.unwrap();
    assert!(requests[0].contains("x-amz-target: awscognitoidentityproviderservice.initiateauth\r\n"), "{}", requests[0]);
    assert!(requests[0].contains(r#""REFRESH_TOKEN":"r1""#), "{}", requests[0]);
    assert!(requests[1].contains("authorization: bearer t1\r\n"), "{}", requests[1]);
}

#[tokio::test]
async fn retries_once_with_a_new_token_after_a_401() {
    let (base, requests) =
        serve(vec![tokens("t1"), ("401 Unauthorized", "{}".into()), tokens("t2"), current_user()]).await;
    let client = Client::with_base_url(Authenticator::from_refresh_token_at(&base, "r1"), base);
    client.current_user().await.unwrap();

    let requests = requests.await.unwrap();
    assert!(requests[1].contains("authorization: bearer t1\r\n"));
    assert!(requests[2].contains("initiateauth"), "{}", requests[2]);
    assert!(requests[3].contains("authorization: bearer t2\r\n"), "{}", requests[3]);
}

#[tokio::test]
async fn a_rejected_refresh_token_means_logging_in_again() {
    let body = r#"{"__type": "NotAuthorizedException", "message": "Refresh Token has expired"}"#;
    let (base, _) = serve(vec![("400 Bad Request", body.into())]).await;
    let client = Client::with_base_url(Authenticator::from_refresh_token_at(&base, "old"), base);
    let error = client.current_user().await.unwrap_err();
    assert!(error.is_auth(), "{error}");
}

#[tokio::test]
async fn a_throttled_refresh_keeps_the_session() {
    let body = r#"{"__type": "TooManyRequestsException", "message": "Rate exceeded"}"#;
    let (base, _) = serve(vec![("400 Bad Request", body.into())]).await;
    let client = Client::with_base_url(Authenticator::from_refresh_token_at(&base, "r1"), base);
    let error = client.current_user().await.unwrap_err();
    assert!(!error.is_auth(), "{error}");
}

#[tokio::test]
async fn logs_in_with_srp_and_an_sms_code() {
    let verifier = r#"{"ChallengeName": "PASSWORD_VERIFIER", "ChallengeParameters": {
        "SALT": "abcd", "SRP_B": "02", "SECRET_BLOCK": "c2VjcmV0", "USER_ID_FOR_SRP": "uid", "USERNAME": "uid"}}"#;
    let mfa = r#"{"ChallengeName": "SMS_MFA", "Session": "s1", "ChallengeParameters": {"USER_ID_FOR_SRP": "uid"}}"#;
    let done = r#"{"AuthenticationResult": {"AccessToken": "t", "RefreshToken": "r", "ExpiresIn": 3600}}"#;
    let (base, requests) =
        serve(vec![("200 OK", verifier.into()), ("200 OK", mfa.into()), ("200 OK", done.into())]).await;

    let LoginOutcome::MfaRequired(challenge) = Authenticator::login_at(&base, "me@example.com", "secret").await.unwrap()
    else {
        panic!("expected an MFA challenge")
    };
    assert_eq!(challenge.kind(), MfaKind::Sms);
    let auth = challenge.respond("123456").await.unwrap();
    assert_eq!(auth.refresh_token(), "r");

    let requests = requests.await.unwrap();
    assert!(requests[0].contains(r#""USERNAME":"me@example.com""#) && requests[0].contains("SRP_A"), "{}", requests[0]);
    assert!(!requests.iter().any(|r| r.contains("secret")), "the password is never sent");
    assert!(requests[1].contains("PASSWORD_CLAIM_SIGNATURE"), "{}", requests[1]);
    assert!(requests[2].contains(r#""SMS_MFA_CODE":"123456""#) && requests[2].contains(r#""Session":"s1""#), "{}", requests[2]);
}
