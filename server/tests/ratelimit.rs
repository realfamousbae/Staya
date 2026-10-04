//! Лимиты частоты на настоящем приложении (задача 3.6).

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{DOMAIN, TestDb, account};
use serde_json::json;
use staya_server::db;
use staya_server::http::{AppState, Config, app};
use staya_server::ratelimit::{AUTH_PER_IP, PER_SESSION};
use tower::ServiceExt;

async fn proxied_app() -> Option<(TestDb, Router)> {
    let t = TestDb::new().await?;
    db::migrate(&t.pool, db::MIGRATIONS).await.unwrap();
    let state = AppState::new(
        t.pool.clone(),
        Config {
            domain: DOMAIN.into(),
            invite_code: None,
            trust_proxy: true,
        },
    );
    Some((t, app(state)))
}

async fn challenge_from(app: &Router, ip: &str) -> StatusCode {
    let req = Request::post("/v1/auth/challenge")
        .header("Content-Type", "application/json")
        // Клиент подставил своё значение, Caddy дописал настоящий адрес в конец.
        .header("X-Forwarded-For", format!("10.9.9.9, {ip}"))
        .body(Body::from(
            json!({"account_id": "AAAAAAAAAAAAAAAAAAAAAA"}).to_string(),
        ))
        .unwrap();
    app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn login_is_limited_per_client_ip() {
    let Some((t, app)) = proxied_app().await else {
        return;
    };
    let burst = AUTH_PER_IP.burst as usize;
    for _ in 0..burst {
        // Аккаунта нет — 404, но запрос посчитан.
        assert_eq!(
            challenge_from(&app, "203.0.113.7").await,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        challenge_from(&app, "203.0.113.7").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Другой клиент за тем же прокси не страдает.
    assert_eq!(
        challenge_from(&app, "198.51.100.4").await,
        StatusCode::NOT_FOUND
    );
    t.drop().await;
}

#[tokio::test]
async fn session_is_limited() {
    let Some((t, app)) = common::setup(None).await else {
        return;
    };
    let (_, token) = account(&app, 1).await;
    let count = || {
        Request::get("/v1/keys/count")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap()
    };
    for _ in 0..PER_SESSION.burst as usize {
        assert_eq!(
            app.clone().oneshot(count()).await.unwrap().status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        app.clone().oneshot(count()).await.unwrap().status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    t.drop().await;
}
