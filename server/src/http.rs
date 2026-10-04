//! HTTP-приложение сервера и журнал запросов без персональных данных.
//!
//! В журнал попадают только метод, шаблон маршрута (`/v1/slots/{recipient}`, а не
//! сам путь с ID аккаунта), код ответа и время. Никаких IP, заголовков (в том
//! числе `Authorization` и `X-Forwarded-For` от Caddy), тел и параметров.

use std::time::Instant;

use std::sync::Arc;

use axum::extract::{FromRequest, MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use deadpool_postgres::Pool;
use serde::de::DeserializeOwned;

use crate::auth;

/// Настройки сервера, не меняющиеся во время работы.
pub struct Config {
    /// Имя сервера в подписи входа (protocol §4.2): хост, к которому подключаются клиенты.
    pub domain: String,
    /// Код приглашения на регистрацию; `None` — регистрация открыта.
    pub invite_code: Option<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub config: Arc<Config>,
}

pub fn app(state: AppState) -> Router {
    with_request_log(
        Router::new()
            .route("/health", get(health))
            .route("/v1/accounts", post(auth::register))
            .route("/v1/auth/challenge", post(auth::challenge))
            .route("/v1/auth/verify", post(auth::verify_challenge))
            .with_state(state),
    )
}

/// JSON-тело запроса. При ошибке разбора — голый код ответа: текст ошибки serde
/// мог бы повторить кусок присланных данных.
pub struct JsonBody<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for JsonBody<T> {
    type Rejection = StatusCode;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(req, state)
            .await
            .map(|Json(v)| Self(v))
            .map_err(|e| e.status())
    }
}

/// Внутренняя ошибка (база, пул): в журнал — только факт, без текста с данными.
pub fn internal<E>(_: E) -> StatusCode {
    tracing::warn!("internal error");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// Оборачивает маршруты журналом запросов. Слой ставится последним, чтобы видеть
/// шаблон маршрута.
pub fn with_request_log(router: Router) -> Router {
    router.layer(middleware::from_fn(log_request))
}

async fn log_request(req: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = req.method().clone();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or("<unmatched>", MatchedPath::as_str)
        .to_owned();
    let response = next.run(req).await;
    tracing::info!(
        %method,
        route,
        status = response.status().as_u16(),
        ms = started.elapsed().as_millis() as u64,
        "request"
    );
    response
}

/// 200, если база отвечает, иначе 503 — без текста ошибки.
async fn health(State(state): State<AppState>) -> StatusCode {
    if crate::db::ping(&state.pool).await {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
