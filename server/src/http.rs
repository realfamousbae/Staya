//! HTTP-приложение сервера и журнал запросов без персональных данных.
//!
//! В журнал попадают только метод, шаблон маршрута (`/v1/slots/{recipient}`, а не
//! сам путь с ID аккаунта), код ответа и время. Никаких IP, заголовков (в том
//! числе `Authorization` и `X-Forwarded-For` от Caddy), тел и параметров.

use std::time::Instant;

use axum::Router;
use axum::extract::{MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use deadpool_postgres::Pool;

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
}

pub fn app(state: AppState) -> Router {
    with_request_log(
        Router::new()
            .route("/health", get(health))
            .with_state(state),
    )
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
