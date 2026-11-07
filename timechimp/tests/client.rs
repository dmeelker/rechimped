mod common;

use chrono::NaiveDate;
use timechimp::{Authenticator, Client, Error};

async fn serve_once(status: &'static str, body: String) -> (String, impl Future<Output = String>) {
    let (base, requests) = common::serve(vec![(status, body)]).await;
    (base, async { requests.await.unwrap().remove(0) })
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

    let request = request.await.to_lowercase();
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
