//! Почтовые ящики на настоящем PostgreSQL (задача 3.4, protocol §8.1–8.3).

mod common;

use axum::Router;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{Client, account, call, call_auth, setup};
use serde_json::{Value, json};
use staya_proto::consts::{CONTROL_QUEUE_LIMIT, LOCATION_ENVELOPE_LEN};

fn env(to: &Client, kind: &str, len: usize, fill: u8) -> Value {
    json!({"to": to.id.to_b64(), "kind": kind, "data": STANDARD.encode(vec![fill; len])})
}

async fn send(app: &Router, token: &str, envs: Vec<Value>) -> (StatusCode, Vec<String>) {
    let (s, v) = call_auth(
        app,
        "POST",
        "/v1/envelopes",
        Some(json!({"envelopes": envs})),
        Some(token),
    )
    .await;
    let statuses = v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| r["status"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default();
    (s, statuses)
}

async fn mailbox(app: &Router, token: &str) -> Value {
    let (s, v) = call_auth(app, "GET", "/v1/mailbox", None, Some(token)).await;
    assert_eq!(s, StatusCode::OK);
    v
}

#[tokio::test]
async fn slots_upsert_queue_keeps_order_and_sender_comes_from_session() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (alice, at) = account(&app, 1).await;
    let (bob, bt) = account(&app, 2).await;
    let (s, r) = send(
        &app,
        &at,
        vec![
            env(&bob, "control", 512, 1),
            env(&bob, "location", LOCATION_ENVELOPE_LEN, 2),
            env(&bob, "control", 1280, 3),
            env(&bob, "location", LOCATION_ENVELOPE_LEN, 4),
        ],
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r, ["accepted"; 4]);

    let m = mailbox(&app, &bt).await;
    let control = m["control"].as_array().unwrap();
    assert_eq!(control.len(), 2);
    assert!(control[0]["seq"].as_i64() < control[1]["seq"].as_i64());
    assert_eq!(control[0]["from"], json!(alice.id.to_b64()));
    assert_eq!(
        STANDARD
            .decode(control[1]["data"].as_str().unwrap())
            .unwrap()
            .len(),
        1280
    );
    let slots = m["locations"].as_array().unwrap();
    assert_eq!(slots.len(), 1, "location slot is upserted");
    assert_eq!(
        slots[0]["data"],
        json!(STANDARD.encode(vec![4u8; LOCATION_ENVELOPE_LEN]))
    );
    // Отправитель свой ящик не видит.
    let mine = mailbox(&app, &at).await;
    assert!(
        mine["control"].as_array().unwrap().is_empty()
            && mine["locations"].as_array().unwrap().is_empty()
    );
    t.drop().await;
}

#[tokio::test]
async fn bad_envelopes_are_rejected_individually() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (_, at) = account(&app, 1).await;
    let (bob, _) = account(&app, 2).await;
    let ghost = Client::new(7);
    let (_, r) = send(
        &app,
        &at,
        vec![
            env(&bob, "location", 161, 0),
            env(&bob, "control", 513, 0),
            env(&ghost, "control", 512, 0),
            env(&bob, "control", 9472, 0),
        ],
    )
    .await;
    assert_eq!(r, ["rejected", "rejected", "rejected", "accepted"]);

    let too_many: Vec<Value> = (0..257)
        .map(|_| env(&bob, "location", LOCATION_ENVELOPE_LEN, 0))
        .collect();
    assert_eq!(
        send(&app, &at, too_many).await.0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let (s, _) = call(
        &app,
        "POST",
        "/v1/envelopes",
        Some(json!({"envelopes": []})),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    t.drop().await;
}

#[tokio::test]
async fn queue_limit_holds_under_parallel_senders() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (bob, bt) = account(&app, 2).await;
    let mut senders = Vec::new();
    for seed in 10..14 {
        senders.push(account(&app, seed).await.1);
    }
    let handles: Vec<_> = senders
        .into_iter()
        .map(|token| {
            let (app, to) = (app.clone(), Client::new(2));
            tokio::spawn(async move {
                let envs = (0..60).map(|_| env(&to, "control", 512, 0)).collect();
                send(&app, &token, envs).await.1
            })
        })
        .collect();
    let mut accepted = 0;
    for h in handles {
        accepted += h.await.unwrap().iter().filter(|s| *s == "accepted").count();
    }
    assert_eq!(accepted, CONTROL_QUEUE_LIMIT);
    assert_eq!(
        mailbox(&app, &bt).await["control"]
            .as_array()
            .unwrap()
            .len(),
        CONTROL_QUEUE_LIMIT
    );
    let _ = bob;
    t.drop().await;
}

#[tokio::test]
async fn crossing_senders_do_not_deadlock() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (x, xt) = account(&app, 1).await;
    let (y, yt) = account(&app, 2).await;
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let app = app.clone();
            let (token, first, second) = if i % 2 == 0 {
                (xt.clone(), Client::new(2), Client::new(1))
            } else {
                (yt.clone(), Client::new(1), Client::new(2))
            };
            tokio::spawn(async move {
                send(
                    &app,
                    &token,
                    vec![
                        env(&first, "location", LOCATION_ENVELOPE_LEN, 0),
                        env(&second, "location", LOCATION_ENVELOPE_LEN, 0),
                    ],
                )
                .await
                .0
            })
        })
        .collect();
    for h in handles {
        assert_eq!(h.await.unwrap(), StatusCode::OK);
    }
    let _ = (x, y);
    t.drop().await;
}

#[tokio::test]
async fn ack_and_slot_delete_touch_only_own_data() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (alice, at) = account(&app, 1).await;
    let (bob, bt) = account(&app, 2).await;
    send(
        &app,
        &at,
        vec![
            env(&bob, "control", 512, 0),
            env(&bob, "location", LOCATION_ENVELOPE_LEN, 0),
        ],
    )
    .await;
    send(
        &app,
        &bt,
        vec![env(&alice, "location", LOCATION_ENVELOPE_LEN, 0)],
    )
    .await;
    let seq = mailbox(&app, &bt).await["control"][0]["seq"].clone();

    // Алиса не может подтвердить чужую очередь и удалить чужой слот.
    call_auth(
        &app,
        "POST",
        "/v1/mailbox/ack",
        Some(json!({"seqs": [seq]})),
        Some(&at),
    )
    .await;
    let path = format!("/v1/slots/{}", alice.id.to_b64());
    call_auth(&app, "DELETE", &path, None, Some(&at)).await;
    let m = mailbox(&app, &bt).await;
    assert_eq!(m["control"].as_array().unwrap().len(), 1);
    assert_eq!(
        mailbox(&app, &at).await["locations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let (s, _) = call_auth(
        &app,
        "POST",
        "/v1/mailbox/ack",
        Some(json!({"seqs": [seq]})),
        Some(&bt),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(
        mailbox(&app, &bt).await["control"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Своё удаление слота — идемпотентно.
    let path = format!("/v1/slots/{}", bob.id.to_b64());
    for _ in 0..2 {
        assert_eq!(
            call_auth(&app, "DELETE", &path, None, Some(&at)).await.0,
            StatusCode::NO_CONTENT
        );
    }
    assert!(
        mailbox(&app, &bt).await["locations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        call_auth(&app, "DELETE", "/v1/slots/not-an-id", None, Some(&at))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    t.drop().await;
}

#[tokio::test]
async fn expired_messages_and_slots_disappear() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (_, at) = account(&app, 1).await;
    let (bob, bt) = account(&app, 2).await;
    send(
        &app,
        &at,
        vec![
            env(&bob, "control", 512, 0),
            env(&bob, "location", LOCATION_ENVELOPE_LEN, 0),
        ],
    )
    .await;
    let client = t.pool.get().await.unwrap();
    client
        .batch_execute(
            "UPDATE control_queue SET created_at = now() - interval '30 days 1 second';
             UPDATE location_slots SET updated_at = now() - interval '72 hours 1 second';",
        )
        .await
        .unwrap();
    let m = mailbox(&app, &bt).await;
    assert!(
        m["control"].as_array().unwrap().is_empty()
            && m["locations"].as_array().unwrap().is_empty()
    );
    assert_eq!(
        staya_server::envelopes::purge_expired(&t.pool)
            .await
            .unwrap(),
        2
    );
    drop(client);
    t.drop().await;
}
