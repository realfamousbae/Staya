//! Живая доставка по WebSocket через настоящий TCP-сервер (задача 3.5).

mod common;

use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{Client, account, call_auth, setup};
use futures_util::StreamExt;
use serde_json::{Value, json};
use staya_proto::consts::LOCATION_ENVELOPE_LEN;
use staya_server::live::MAX_CONNECTIONS_PER_ACCOUNT;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("ws://{addr}/v1/ws")
}

async fn connect(url: &str, token: Option<&str>) -> Result<Socket, StatusCode> {
    let mut req = url.into_client_request().unwrap();
    if let Some(t) = token {
        req.headers_mut()
            .insert("Authorization", format!("Bearer {t}").parse().unwrap());
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((socket, _)) => Ok(socket),
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            Err(StatusCode::from_u16(resp.status().as_u16()).unwrap())
        }
        Err(e) => panic!("{e}"),
    }
}

async fn next_event(socket: &mut Socket) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("event within 5 s")
            .unwrap()
            .unwrap();
        if let Message::Text(text) = msg {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

fn env(to: &Client, kind: &str, len: usize, fill: u8) -> Value {
    json!({"to": to.id.to_b64(), "kind": kind, "data": STANDARD.encode(vec![fill; len])})
}

#[tokio::test]
async fn new_envelopes_arrive_live_after_commit() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (alice, at) = account(&app, 1).await;
    let (bob, bt) = account(&app, 2).await;
    let url = serve(app.clone()).await;
    let mut socket = connect(&url, Some(&bt)).await.unwrap();

    let envs = vec![
        env(&bob, "control", 512, 1),
        env(&bob, "location", LOCATION_ENVELOPE_LEN, 2),
    ];
    let (s, _) = call_auth(
        &app,
        "POST",
        "/v1/envelopes",
        Some(json!({"envelopes": envs})),
        Some(&at),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    let control = next_event(&mut socket).await;
    assert_eq!(control["type"], "control");
    assert_eq!(control["from"], json!(alice.id.to_b64()));
    let location = next_event(&mut socket).await;
    assert_eq!(location["type"], "location");
    assert_eq!(
        location["data"],
        json!(STANDARD.encode(vec![2u8; LOCATION_ENVELOPE_LEN]))
    );

    // То же лежит в ящике, номер совпадает — подтверждение как обычно.
    let (_, m) = call_auth(&app, "GET", "/v1/mailbox", None, Some(&bt)).await;
    assert_eq!(m["control"][0]["seq"], control["seq"]);

    // Отклонённое не рассылается.
    let (_, r) = call_auth(
        &app,
        "POST",
        "/v1/envelopes",
        Some(json!({"envelopes": [env(&bob, "control", 13, 0)]})),
        Some(&at),
    )
    .await;
    assert_eq!(r["results"][0]["status"], "rejected");
    assert!(
        tokio::time::timeout(Duration::from_millis(300), socket.next())
            .await
            .is_err()
    );
    socket.close(None).await.unwrap();
    t.drop().await;
}

#[tokio::test]
async fn needs_session_and_limits_connections() {
    let Some((t, app)) = setup(None).await else {
        return;
    };
    let (_, bt) = account(&app, 2).await;
    let url = serve(app).await;
    assert_eq!(
        connect(&url, None).await.err(),
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        connect(&url, Some("bm9wZQ==")).await.err(),
        Some(StatusCode::UNAUTHORIZED)
    );

    let mut open = Vec::new();
    for _ in 0..MAX_CONNECTIONS_PER_ACCOUNT {
        open.push(connect(&url, Some(&bt)).await.unwrap());
    }
    assert_eq!(
        connect(&url, Some(&bt)).await.err(),
        Some(StatusCode::TOO_MANY_REQUESTS)
    );

    // Закрытое соединение освобождает место.
    open.pop().unwrap().close(None).await.unwrap();
    let mut ok = false;
    for _ in 0..50 {
        if let Ok(s) = connect(&url, Some(&bt)).await {
            open.push(s);
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ok, "a closed connection must free its slot");
    t.drop().await;
}
