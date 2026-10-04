//! Каталог ключей на настоящем PostgreSQL (задача 3.3, protocol §4.3).

mod common;

use std::collections::BTreeSet;

use axum::Router;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{Client, call, call_auth, login, setup};
use ed25519_dalek::{Signature, Signer, VerifyingKey};
use serde_json::{Value, json};
use staya_proto::signing;
use staya_server::keys::MAX_ONE_TIME_KEYS;

fn otk(c: &Client, n: u16) -> Value {
    let mut key = [0u8; 32];
    key[..2].copy_from_slice(&n.to_be_bytes());
    key[2] = c.id.0[0];
    let sig = c.key.sign(&signing::one_time_key(&key));
    json!({"key": STANDARD.encode(key), "signature": STANDARD.encode(sig.to_bytes())})
}

fn fallback(c: &Client) -> Value {
    let key = [0xfb; 32];
    let sig = c.key.sign(&signing::fallback_key(&key));
    json!({"key": STANDARD.encode(key), "signature": STANDARD.encode(sig.to_bytes())})
}

async fn account(app: &Router, seed: u8) -> (Client, String) {
    let c = Client::new(seed);
    let (s, _) = call(app, "POST", "/v1/accounts", Some(c.register_body(None))).await;
    assert_eq!(s, StatusCode::CREATED);
    let token = login(app, &c).await;
    (c, token)
}

async fn count(app: &Router, token: &str) -> u64 {
    let (s, v) = call_auth(app, "GET", "/v1/keys/count", None, Some(token)).await;
    assert_eq!(s, StatusCode::OK);
    v["one_time_keys"].as_u64().unwrap()
}

async fn claim(app: &Router, token: &str, target: &Client) -> (StatusCode, Value) {
    call_auth(
        app,
        "POST",
        "/v1/keys/claim",
        Some(json!({"account_id": target.id.to_b64()})),
        Some(token),
    )
    .await
}

#[tokio::test]
async fn publish_claim_and_fallback() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (alice, at) = account(&app, 1).await;
    let (_, bt) = account(&app, 2).await;
    let keys: Vec<Value> = (0..3).map(|n| otk(&alice, n)).collect();
    let body = json!({"one_time_keys": keys, "fallback_key": fallback(&alice)});
    let (s, _) = call_auth(&app, "PUT", "/v1/keys", Some(body.clone()), Some(&at)).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    // Повтор той же публикации не дублирует ключи.
    call_auth(&app, "PUT", "/v1/keys", Some(body), Some(&at)).await;
    assert_eq!(count(&app, &at).await, 3);

    let sk = VerifyingKey::from_bytes(&alice.sk()).unwrap();
    for n in 0..3u16 {
        let (s, v) = claim(&app, &bt, &alice).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["is_fallback"], json!(false));
        // Выдаются по порядку публикации, подпись — настоящая.
        assert_eq!(v["key"], otk(&alice, n));
        let key: [u8; 32] = STANDARD
            .decode(v["key"]["key"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let sig: [u8; 64] = STANDARD
            .decode(v["key"]["signature"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        sk.verify_strict(&signing::one_time_key(&key), &Signature::from_bytes(&sig))
            .unwrap();
    }
    assert_eq!(count(&app, &at).await, 0);
    // OTK кончились — fallback, и он не расходуется.
    for _ in 0..2 {
        let (s, v) = claim(&app, &bt, &alice).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["is_fallback"], json!(true));
        assert_eq!(v["key"], fallback(&alice));
    }
    t.drop().await;
}

#[tokio::test]
async fn rejects_bad_signatures_and_overflow_atomically() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (alice, at) = account(&app, 1).await;
    let mallory = Client::new(9);

    // Чужая подпись в середине — отказ всему запросу.
    let body = json!({"one_time_keys": [otk(&alice, 1), otk(&mallory, 2)], "fallback_key": null});
    let (s, _) = call_auth(&app, "PUT", "/v1/keys", Some(body), Some(&at)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Подпись OTK не годится как подпись fallback-ключа (разные контексты).
    let mut fb = otk(&alice, 1);
    fb["key"] = json!(STANDARD.encode({
        let mut k = [0u8; 32];
        k[..2].copy_from_slice(&1u16.to_be_bytes());
        k[2] = alice.id.0[0];
        k
    }));
    let body = json!({"one_time_keys": [], "fallback_key": fb});
    let (s, _) = call_auth(&app, "PUT", "/v1/keys", Some(body), Some(&at)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(count(&app, &at).await, 0);

    let too_many: Vec<Value> = (0..=MAX_ONE_TIME_KEYS as u16)
        .map(|n| otk(&alice, n))
        .collect();
    let body = json!({"one_time_keys": too_many, "fallback_key": null});
    let (s, _) = call_auth(&app, "PUT", "/v1/keys", Some(body), Some(&at)).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(count(&app, &at).await, 0, "nothing is stored on overflow");
    t.drop().await;
}

#[tokio::test]
async fn parallel_claims_get_distinct_keys() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (alice, at) = account(&app, 1).await;
    let (_, bt) = account(&app, 2).await;
    let keys: Vec<Value> = (0..20).map(|n| otk(&alice, n)).collect();
    call_auth(
        &app,
        "PUT",
        "/v1/keys",
        Some(json!({"one_time_keys": keys, "fallback_key": fallback(&alice)})),
        Some(&at),
    )
    .await;

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let (app, bt, target) = (app.clone(), bt.clone(), Client::new(1));
            tokio::spawn(async move { claim(&app, &bt, &target).await })
        })
        .collect();
    let mut results = Vec::new();
    for h in handles {
        results.push(h.await.unwrap());
    }
    let keys: BTreeSet<String> = results
        .iter()
        .map(|(s, v)| {
            assert_eq!(*s, StatusCode::OK);
            assert_eq!(v["is_fallback"], json!(false));
            v["key"]["key"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(keys.len(), 20, "a one-time key was handed out twice");
    t.drop().await;
}

#[tokio::test]
async fn needs_session_and_known_account() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (alice, at) = account(&app, 1).await;
    let (s, _) = call(&app, "GET", "/v1/keys/count", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(
        &app,
        "POST",
        "/v1/keys/claim",
        Some(json!({"account_id": alice.id.to_b64()})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // Ни OTK, ни fallback / нет аккаунта — 404.
    assert_eq!(claim(&app, &at, &alice).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        claim(&app, &at, &Client::new(7)).await.0,
        StatusCode::NOT_FOUND
    );
    t.drop().await;
}

#[tokio::test]
async fn real_core_keys_are_accepted() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("staya.db").to_string_lossy().into_owned();
    let core = staya_core::api::StayaCore::open(path, vec![3; 32]).unwrap();
    let id = core.identity().unwrap().account_id;
    let body: Value = serde_json::from_str(&core.register_request(None).unwrap()).unwrap();
    call(&app, "POST", "/v1/accounts", Some(body)).await;
    let (_, ch) = call(
        &app,
        "POST",
        "/v1/auth/challenge",
        Some(json!({"account_id": id})),
    )
    .await;
    let nonce = STANDARD.decode(ch["nonce"].as_str().unwrap()).unwrap();
    let sig = core
        .sign_auth(common::DOMAIN.into(), nonce.clone())
        .unwrap();
    let (_, v) = call(
        &app,
        "POST",
        "/v1/auth/verify",
        Some(json!({
            "account_id": id, "nonce": STANDARD.encode(&nonce), "signature": STANDARD.encode(sig),
        })),
    )
    .await;
    let token = v["token"].as_str().unwrap().to_owned();

    let publish: Value =
        serde_json::from_str(&core.keys_to_publish(0, 1_700_000_000).unwrap().unwrap()).unwrap();
    let (s, _) = call_auth(&app, "PUT", "/v1/keys", Some(publish), Some(&token)).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    core.mark_keys_published().unwrap();
    assert_eq!(
        count(&app, &token).await,
        staya_proto::consts::OTK_TARGET as u64
    );
    t.drop().await;
}
