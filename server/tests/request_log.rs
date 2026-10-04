//! Журнал запросов не содержит IP, ID аккаунтов из пути и заголовков (задача 3.1).

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::delete;
use staya_server::http::with_request_log;
use tower::ServiceExt;

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn request_log_has_route_template_but_no_ip_or_ids() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let app = with_request_log(Router::new().route(
        "/v1/slots/{recipient}",
        delete(|| async { StatusCode::NO_CONTENT }),
    ));
    let account = "q83vEjRWeJA1qrzN7_8AEQ";
    let token = "c2VjcmV0LXRva2VuLXZhbHVl";
    let response = app
        .oneshot(
            Request::delete(format!("/v1/slots/{account}?probe=query-value"))
                .header("X-Forwarded-For", "203.0.113.7")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from("body-content"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("/v1/slots/{recipient}"), "{log}");
    assert!(log.contains("204"), "{log}");
    for secret in [account, "203.0.113.7", token, "query-value", "body-content"] {
        assert!(!log.contains(secret), "log leaks {secret}: {log}");
    }
}
