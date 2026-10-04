//! Регистрация и вход на настоящем PostgreSQL (задача 3.2, protocol §4.1–4.2).

mod common;

use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{Client, DOMAIN, call, call_auth, login, setup};
use serde_json::{Value, json};

#[tokio::test]
async fn register_is_idempotent_and_keys_are_bound() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let alice = Client::new(1);
    let (s, _) = call(
        &app,
        "POST",
        "/v1/accounts",
        Some(alice.register_body(None)),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = call(
        &app,
        "POST",
        "/v1/accounts",
        Some(alice.register_body(None)),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "same keys again is fine");

    // Тот же ID с другими ключами — нельзя перехватить аккаунт.
    let mut mallory = Client::new(9);
    mallory.id = alice.id;
    let (s, _) = call(
        &app,
        "POST",
        "/v1/accounts",
        Some(mallory.register_body(None)),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    t.drop().await;
}

#[tokio::test]
async fn register_rejects_bad_signature_and_garbage() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let alice = Client::new(1);
    let mut body = alice.register_body(None);
    body["ik"] = json!(STANDARD.encode([0u8; 32]));
    let (s, _) = call(&app, "POST", "/v1/accounts", Some(body)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, text) = call(
        &app,
        "POST",
        "/v1/accounts",
        Some(json!({"account_id": "secret-looking-value"})),
    )
    .await;
    assert!(s.is_client_error());
    assert!(
        !text.to_string().contains("secret-looking-value"),
        "error echoes input: {text}"
    );
    t.drop().await;
}

#[tokio::test]
async fn invite_code_gates_new_accounts_only() {
    let Some((t, app)) = setup(Some("beta-code")).await else {
        return;
    };
    let alice = Client::new(1);
    let (s, _) = call(
        &app,
        "POST",
        "/v1/accounts",
        Some(alice.register_body(None)),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(
        &app,
        "POST",
        "/v1/accounts",
        Some(alice.register_body(Some("wrong"))),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = call(
        &app,
        "POST",
        "/v1/accounts",
        Some(alice.register_body(Some("beta-code"))),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    // Повтор уже существующего аккаунта код не требует.
    let (s, _) = call(
        &app,
        "POST",
        "/v1/accounts",
        Some(alice.register_body(None)),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    t.drop().await;
}

#[tokio::test]
async fn login_gives_a_working_token() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let alice = Client::new(1);
    call(
        &app,
        "POST",
        "/v1/accounts",
        Some(alice.register_body(None)),
    )
    .await;
    let token = login(&app, &alice).await;
    let (s, who) = call_auth(&app, "GET", "/whoami", None, Some(&token)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(who, Value::String(alice.id.to_b64()));

    // Сервер хранит только хеш: токена в базе нет.
    let client = t.pool.get().await.unwrap();
    let raw = STANDARD.decode(&token).unwrap();
    let stored: i64 = client
        .query_one(
            "SELECT count(*) FROM sessions WHERE token_hash = $1",
            &[&raw.as_slice()],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(stored, 0);
    drop(client);

    let (s, _) = call_auth(&app, "GET", "/whoami", None, Some("bm90LWEtdG9rZW4=")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&app, "GET", "/whoami", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    t.drop().await;
}

#[tokio::test]
async fn challenge_is_single_use_and_bound_to_server_and_account() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (alice, bob) = (Client::new(1), Client::new(2));
    for c in [&alice, &bob] {
        call(&app, "POST", "/v1/accounts", Some(c.register_body(None))).await;
    }
    let challenge = || async {
        let (_, ch) = call(
            &app,
            "POST",
            "/v1/auth/challenge",
            Some(json!({"account_id": alice.id.to_b64()})),
        )
        .await;
        STANDARD.decode(ch["nonce"].as_str().unwrap()).unwrap()
    };

    // Подпись для другого сервера не принимается, и nonce после попытки сгорает.
    let nonce = challenge().await;
    let (s, _) = call(
        &app,
        "POST",
        "/v1/auth/verify",
        Some(alice.verify_body("evil.example", &nonce)),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(
        &app,
        "POST",
        "/v1/auth/verify",
        Some(alice.verify_body(DOMAIN, &nonce)),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "nonce must be burnt");

    // Чужой nonce — нельзя.
    let nonce = challenge().await;
    let (s, _) = call(
        &app,
        "POST",
        "/v1/auth/verify",
        Some(bob.verify_body(DOMAIN, &nonce)),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Повтор удачного входа — нельзя.
    let nonce = challenge().await;
    let body = alice.verify_body(DOMAIN, &nonce);
    assert_eq!(
        call(&app, "POST", "/v1/auth/verify", Some(body.clone()))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", "/v1/auth/verify", Some(body)).await.0,
        StatusCode::UNAUTHORIZED
    );

    // Неизвестный аккаунт — 404: клиент перерегистрируется (§4.1).
    let (s, _) = call(
        &app,
        "POST",
        "/v1/auth/challenge",
        Some(json!({"account_id": Client::new(7).id.to_b64()})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    t.drop().await;
}

#[tokio::test]
async fn expired_challenge_and_session_are_rejected_and_purged() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let alice = Client::new(1);
    call(
        &app,
        "POST",
        "/v1/accounts",
        Some(alice.register_body(None)),
    )
    .await;
    let token = login(&app, &alice).await;
    let (_, ch) = call(
        &app,
        "POST",
        "/v1/auth/challenge",
        Some(json!({"account_id": alice.id.to_b64()})),
    )
    .await;
    let nonce = STANDARD.decode(ch["nonce"].as_str().unwrap()).unwrap();

    let client = t.pool.get().await.unwrap();
    client
        .batch_execute(
            "UPDATE sessions SET expires_at = now() - interval '1 second';
             UPDATE auth_challenges SET expires_at = now() - interval '1 second';",
        )
        .await
        .unwrap();
    let (s, _) = call(
        &app,
        "POST",
        "/v1/auth/verify",
        Some(alice.verify_body(DOMAIN, &nonce)),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call_auth(&app, "GET", "/whoami", None, Some(&token)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    staya_server::auth::purge_expired(&t.pool).await.unwrap();
    let left: i64 = client
        .query_one(
            "SELECT (SELECT count(*) FROM sessions) + (SELECT count(*) FROM auth_challenges)",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(left, 0);
    drop(client);
    t.drop().await;
}

#[tokio::test]
async fn real_core_registers_and_logs_in() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("staya.db").to_string_lossy().into_owned();
    let core = staya_core::api::StayaCore::open(path, vec![3; 32]).unwrap();
    let id = core.identity().unwrap().account_id;

    let body: Value = serde_json::from_str(&core.register_request(None).unwrap()).unwrap();
    let (s, _) = call(&app, "POST", "/v1/accounts", Some(body)).await;
    assert_eq!(s, StatusCode::CREATED);

    let (_, ch) = call(
        &app,
        "POST",
        "/v1/auth/challenge",
        Some(json!({"account_id": id})),
    )
    .await;
    let nonce = STANDARD.decode(ch["nonce"].as_str().unwrap()).unwrap();
    let sig = core.sign_auth(DOMAIN.into(), nonce.clone()).unwrap();
    let body = json!({
        "account_id": id,
        "nonce": STANDARD.encode(&nonce),
        "signature": STANDARD.encode(sig),
    });
    let (s, v) = call(&app, "POST", "/v1/auth/verify", Some(body)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, who) = call_auth(&app, "GET", "/whoami", None, v["token"].as_str()).await;
    assert_eq!(who, Value::String(id));
    t.drop().await;
}
