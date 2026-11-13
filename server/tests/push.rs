//! Push address ownership/retention. These tests never send an envelope or contact Google.
mod common;

use axum::{Router, http::StatusCode};
use common::{DOMAIN, TestDb, account, call_auth};
use serde_json::json;
use sha2::{Digest, Sha256};
use staya_server::{
    db,
    http::{AppState, Config, app},
    push::{FcmClient, PushQueue},
};
use std::{io::Write, sync::OnceLock};

fn fake_client() -> FcmClient {
    static KEY: OnceLock<String> = OnceLock::new();
    let key = KEY.get_or_init(|| {
        let output = std::process::Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
            ])
            .output()
            .expect("openssl test key generator");
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap()
    });
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(serde_json::to_string(&json!({"type":"service_account", "project_id":"staya-test", "client_email":"test@staya-test.iam.gserviceaccount.com", "private_key":key, "private_key_id":"test"})).unwrap().as_bytes()).unwrap();
    FcmClient::from_file(file.path()).unwrap()
}
async fn setup(enabled: bool) -> Option<(TestDb, Router)> {
    let t = TestDb::new().await?;
    db::migrate(&t.pool, db::MIGRATIONS).await.unwrap();
    let mut state = AppState::new(
        t.pool.clone(),
        Config {
            domain: DOMAIN.into(),
            invite_code: None,
            trust_proxy: false,
        },
    );
    if enabled {
        state.push = Some(PushQueue::spawn(
            t.pool.clone(),
            fake_client(),
            state.hub.clone(),
        ));
    }
    Some((t, app(state)))
}

#[tokio::test]
async fn push_registration_is_authenticated_and_token_ownership_can_move_to_a_new_local_account() {
    let Some((t, app)) = setup(true).await else {
        return;
    };
    let (alice, at) = account(&app, 1).await;
    let (bob, bt) = account(&app, 2).await;
    let token = "test-opaque-registration-token";
    let (status, _) = call_auth(&app, "PUT", "/v1/push", Some(json!({"token":token})), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call_auth(
        &app,
        "PUT",
        "/v1/push",
        Some(json!({"token":token})),
        Some(&at),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let row = t
        .pool
        .get()
        .await
        .unwrap()
        .query_one("SELECT account_id, token_hash FROM push_devices", &[])
        .await
        .unwrap();
    assert_eq!(row.get::<_, Vec<u8>>(0), alice.id.0);
    assert_eq!(
        row.get::<_, Vec<u8>>(1),
        Sha256::digest(token.as_bytes()).to_vec()
    );
    // A reinstall/new account on the same device must not retain the old account binding.
    let (status, _) = call_auth(
        &app,
        "PUT",
        "/v1/push",
        Some(json!({"token":token})),
        Some(&bt),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let connection = t.pool.get().await.unwrap();
    let rows = connection
        .query("SELECT account_id FROM push_devices", &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<_, Vec<u8>>(0), bob.id.0);
    drop(connection);
    let (status, _) = call_auth(&app, "DELETE", "/v1/push", None, Some(&at)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        t.pool
            .get()
            .await
            .unwrap()
            .query_one("SELECT count(*) FROM push_devices", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        1
    );
    let (status, _) = call_auth(&app, "DELETE", "/v1/push", None, Some(&bt)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        t.pool
            .get()
            .await
            .unwrap()
            .query_one("SELECT count(*) FROM push_devices", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        0
    );
    t.drop().await;
}

#[tokio::test]
async fn addresses_are_validated_extra_fields_are_rejected_and_old_addresses_are_purged() {
    let Some((t, app)) = setup(true).await else {
        return;
    };
    let (_, token) = account(&app, 3).await;
    for value in [
        "short".to_owned(),
        "x".repeat(4097),
        "test token with spaces".into(),
    ] {
        let (status, _) = call_auth(
            &app,
            "PUT",
            "/v1/push",
            Some(json!({"token":value})),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, _) = call_auth(
        &app,
        "PUT",
        "/v1/push",
        Some(json!({"token":"test-opaque-registration-token", "latitude":55})),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = call_auth(
        &app,
        "PUT",
        "/v1/push",
        Some(json!({"token":"test-opaque-registration-token"})),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    t.pool
        .get()
        .await
        .unwrap()
        .execute(
            "UPDATE push_devices SET updated_at=now()-interval '31 days'",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(staya_server::push::purge_expired(&t.pool).await.unwrap(), 1);
    t.drop().await;
}

#[tokio::test]
async fn capabilities_report_disabled_delivery_but_opt_out_still_works() {
    let Some((t, app)) = setup(false).await else {
        return;
    };
    let (alice, token) = account(&app, 4).await;
    let (status, body) = call_auth(&app, "GET", "/v1/capabilities", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["fcm"], false);
    let (status, _) = call_auth(
        &app,
        "PUT",
        "/v1/push",
        Some(json!({"token":"test-opaque-registration-token"})),
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let digest = Sha256::digest(b"test-opaque-registration-token").to_vec();
    t.pool
        .get()
        .await
        .unwrap()
        .execute(
            "INSERT INTO push_devices(account_id,token,token_hash) VALUES($1,$2,$3)",
            &[
                &alice.id.0.as_slice(),
                &"test-opaque-registration-token",
                &digest,
            ],
        )
        .await
        .unwrap();
    let (status, _) = call_auth(&app, "DELETE", "/v1/push", None, Some(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        t.pool
            .get()
            .await
            .unwrap()
            .query_one("SELECT count(*) FROM push_devices", &[])
            .await
            .unwrap()
            .get::<_, i64>(0),
        0
    );
    t.drop().await;
}
