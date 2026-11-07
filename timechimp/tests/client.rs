//! `Client` against a local one-shot HTTP server: request shape, decoding and error mapping.

use chrono::NaiveDate;
use timechimp::{Authenticator, Client, Error};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Serves one request with `status` and `body`; returns the base URL and the raw request.
async fn serve_once(status: &'static str, body: String) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/api/", listener.local_addr().unwrap());
    let request = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0, "connection closed before the request was complete");
            request.extend_from_slice(&buf[..n]);
        }
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (base, request)
}

fn client(base: String) -> Client {
    Client::with_base_url(Authenticator::with_access_token("test-token"), base)
}

#[tokio::test]
async fn fetches_and_decodes_a_week() {
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/time_entries.json")).unwrap();
    let (base, request) = serve_once("200 OK", fixture).await;

    let monday = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap();
    let entries = client(base).time_entries_for_week(1, monday).await.unwrap();

    let request = request.await.unwrap().to_lowercase();
    assert!(request.starts_with("get /api/time/week/1/2026-11-02 http/1.1\r\n"), "{request}");
    assert!(request.contains("authorization: bearer test-token\r\n"), "{request}");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].start_end.as_deref(), Some("09:00-10:30"));
}

#[tokio::test]
async fn server_errors_are_not_auth_errors() {
    let (base, _) = serve_once("500 Internal Server Error", "{}".into()).await;
    let error = client(base).current_user().await.unwrap_err();
    assert!(matches!(error, Error::Api { status, .. } if status.as_u16() == 500), "{error}");
    assert!(!error.is_auth());
}

#[tokio::test]
async fn unexpected_shapes_are_decode_errors() {
    let (base, _) = serve_once("200 OK", "{\"not\": \"a list\"}".into()).await;
    let error = client(base).projects().await.unwrap_err();
    assert!(matches!(error, Error::Decode { ref url, .. } if url.ends_with("/api/project/uiselect")), "{error}");
}
