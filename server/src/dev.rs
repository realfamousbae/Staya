//! Dev-сервер (задача 2.10): ящики в памяти, без входа и TLS, только loopback.
//!
//! Отправитель — `Authorization: Bearer <account_id>` (base64url): та же форма
//! запроса, что с настоящим токеном (protocol §4.2), только без проверки.
//! Плюс `/dev/invite` — место встречи для тестов в CI: тестовый собеседник кладёт
//! туда приглашение, приложение на симуляторе или эмуляторе забирает.

use std::sync::{Arc, Mutex};

use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use staya_proto::AccountId;
use staya_proto::api::{
    AckRequest, ClaimRequest, ClaimResponse, KeyCountResponse, MailboxResponse, PublishKeysRequest,
    SendRequest, SendResponse,
};

use crate::mailbox::Mailbox;

#[derive(Default)]
struct Inner {
    mailbox: Mailbox,
    invite: Option<String>,
}

#[derive(Clone, Default)]
pub struct DevState(Arc<Mutex<Inner>>);

impl DevState {
    fn with<T>(&self, f: impl FnOnce(&mut Inner) -> T) -> T {
        let mut g = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut g)
    }
}

/// Отправитель запроса из заголовка `Authorization`.
pub struct Caller(AccountId);

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .and_then(|id| AccountId::from_b64(id).ok())
            .map(Caller)
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

pub fn app(state: DevState) -> Router {
    Router::new()
        .route("/v1/keys", put(publish_keys))
        .route("/v1/keys/claim", post(claim))
        .route("/v1/keys/count", get(key_count))
        .route("/v1/envelopes", post(send))
        .route("/v1/mailbox", get(mailbox))
        .route("/v1/mailbox/ack", post(ack))
        .route("/v1/slots/{recipient}", delete(delete_slot))
        .route("/dev/invite", put(put_invite).get(get_invite))
        .with_state(state)
}

async fn publish_keys(
    State(s): State<DevState>,
    Caller(me): Caller,
    Json(req): Json<PublishKeysRequest>,
) -> StatusCode {
    s.with(|i| i.mailbox.publish_keys(me, req));
    StatusCode::NO_CONTENT
}

async fn claim(
    State(s): State<DevState>,
    Caller(_): Caller,
    Json(req): Json<ClaimRequest>,
) -> Result<Json<ClaimResponse>, StatusCode> {
    s.with(|i| i.mailbox.claim(&req.account_id))
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn key_count(State(s): State<DevState>, Caller(me): Caller) -> Json<KeyCountResponse> {
    Json(KeyCountResponse {
        one_time_keys: s.with(|i| i.mailbox.otk_count(&me)),
    })
}

async fn send(
    State(s): State<DevState>,
    Caller(me): Caller,
    Json(req): Json<SendRequest>,
) -> Json<SendResponse> {
    Json(s.with(|i| i.mailbox.send(me, req)))
}

async fn mailbox(State(s): State<DevState>, Caller(me): Caller) -> Json<MailboxResponse> {
    Json(s.with(|i| i.mailbox.mailbox(&me)))
}

async fn ack(
    State(s): State<DevState>,
    Caller(me): Caller,
    Json(req): Json<AckRequest>,
) -> StatusCode {
    s.with(|i| i.mailbox.ack(&me, &req.seqs));
    StatusCode::NO_CONTENT
}

async fn delete_slot(
    State(s): State<DevState>,
    Caller(me): Caller,
    Path(recipient): Path<String>,
) -> StatusCode {
    match AccountId::from_b64(&recipient) {
        Ok(to) => {
            s.with(|i| i.mailbox.delete_slot(me, to));
            StatusCode::NO_CONTENT
        }
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

async fn put_invite(State(s): State<DevState>, body: String) -> StatusCode {
    s.with(|i| i.invite = Some(body));
    StatusCode::NO_CONTENT
}

async fn get_invite(State(s): State<DevState>) -> Result<String, StatusCode> {
    s.with(|i| i.invite.clone()).ok_or(StatusCode::NOT_FOUND)
}
