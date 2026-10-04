//! Отдельная база PostgreSQL на каждый тест.
//!
//! URL сервера — `STAYA_TEST_DATABASE_URL` (например,
//! `postgres://staya:staya@127.0.0.1:54329/staya`). Без него тесты с базой
//! пропускаются локально, но падают в CI — чтобы не «пройти», ничего не проверив.

#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use deadpool_postgres::Pool;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use staya_proto::AccountId;
use staya_proto::signing;
use staya_server::auth::Session;
use staya_server::db;
use staya_server::http::{AppState, Config, app};
use tokio_postgres::NoTls;
use tower::ServiceExt;

pub struct TestDb {
    admin_url: String,
    name: String,
    pub url: String,
    pub pool: Pool,
}

fn base_url() -> Option<String> {
    match std::env::var("STAYA_TEST_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ if std::env::var_os("CI").is_some() => {
            panic!("STAYA_TEST_DATABASE_URL must be set in CI")
        }
        _ => {
            eprintln!("STAYA_TEST_DATABASE_URL is not set: skipping database test");
            None
        }
    }
}

/// URL той же базы-сервера, но с другим именем базы.
fn with_db(url: &str, name: &str) -> String {
    let (prefix, _) = url.rsplit_once('/').expect("database URL with /dbname");
    format!("{prefix}/{name}")
}

async fn admin(url: &str) -> tokio_postgres::Client {
    let (client, conn) = tokio_postgres::connect(url, NoTls).await.expect("connect");
    tokio::spawn(conn);
    client
}

impl TestDb {
    pub async fn new() -> Option<Self> {
        let admin_url = base_url()?;
        let mut rnd = [0u8; 8];
        getrandom::fill(&mut rnd).unwrap();
        let name = format!(
            "staya_test_{}",
            rnd.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        admin(&admin_url)
            .await
            .batch_execute(&format!("CREATE DATABASE {name}"))
            .await
            .expect("create test database");
        let url = with_db(&admin_url, &name);
        let pool = db::pool(&url, 4).unwrap();
        Some(Self {
            admin_url,
            name,
            url,
            pool,
        })
    }

    pub async fn drop(self) {
        self.pool.close();
        admin(&self.admin_url)
            .await
            .batch_execute(&format!("DROP DATABASE {} WITH (FORCE)", self.name))
            .await
            .expect("drop test database");
    }
}

// --- HTTP-клиент тестов ---------------------------------------------------

pub const DOMAIN: &str = "staya.test";

pub struct Client {
    pub id: AccountId,
    pub key: SigningKey,
    pub ik: [u8; 32],
}

impl Client {
    pub fn new(seed: u8) -> Self {
        Self {
            id: AccountId([seed; 16]),
            key: SigningKey::from_bytes(&[seed; 32]),
            ik: [seed.wrapping_add(1); 32],
        }
    }

    pub fn sk(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    pub fn register_body(&self, invite: Option<&str>) -> Value {
        let sig = self
            .key
            .sign(&signing::register(&self.id, &self.ik, &self.sk()));
        json!({
            "account_id": self.id.to_b64(),
            "ik": STANDARD.encode(self.ik),
            "sk": STANDARD.encode(self.sk()),
            "signature": STANDARD.encode(sig.to_bytes()),
            "invite_code": invite,
        })
    }

    pub fn verify_body(&self, domain: &str, nonce: &[u8]) -> Value {
        let n: [u8; 32] = nonce.try_into().unwrap();
        let sig = self.key.sign(&signing::auth(domain, &n, &self.id).unwrap());
        json!({
            "account_id": self.id.to_b64(),
            "nonce": STANDARD.encode(nonce),
            "signature": STANDARD.encode(sig.to_bytes()),
        })
    }
}

pub async fn setup(invite: Option<&str>) -> Option<(TestDb, Router)> {
    let t = TestDb::new().await?;
    db::migrate(&t.pool, db::MIGRATIONS).await.unwrap();
    let state = AppState::new(
        t.pool.clone(),
        Config {
            domain: DOMAIN.into(),
            invite_code: invite.map(Into::into),
            trust_proxy: false,
        },
    );
    let whoami = Router::new()
        .route(
            "/whoami",
            get(|Session(id): Session| async move { id.to_b64() }),
        )
        .with_state(state.clone());
    Some((t, app(state).merge(whoami)))
}

pub async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    call_auth(app, method, path, body, None).await
}

pub async fn call_auth(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req
            .header("Content-Type", "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, value)
}

/// Полный вход: challenge → подпись → токен.
pub async fn login(app: &Router, c: &Client) -> String {
    let (s, ch) = call(
        app,
        "POST",
        "/v1/auth/challenge",
        Some(json!({"account_id": c.id.to_b64()})),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let nonce = STANDARD.decode(ch["nonce"].as_str().unwrap()).unwrap();
    let (s, v) = call(
        app,
        "POST",
        "/v1/auth/verify",
        Some(c.verify_body(DOMAIN, &nonce)),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["token"].as_str().unwrap().to_owned()
}

/// Зарегистрированный и вошедший аккаунт: клиент и токен.
pub async fn account(app: &Router, seed: u8) -> (Client, String) {
    let c = Client::new(seed);
    let (s, _) = call(app, "POST", "/v1/accounts", Some(c.register_body(None))).await;
    assert_eq!(s, StatusCode::CREATED);
    let token = login(app, &c).await;
    (c, token)
}
