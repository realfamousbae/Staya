//! Сборщик метрик этапа 1 (docs/server.md, docs/PLAN.md задачи 1.1–1.5).
//!
//! Принимает от прототипов `POST /probe` с bearer-токеном и дописывает каждую
//! запись строкой JSON в файл дня. Схема строгая (`deny_unknown_fields`): полей с
//! координатами в ней нет, и сервер отвергает запрос, если клиент их пришлёт.
//! IP-адреса и заголовки прокси не читаются и не пишутся.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// Запись больше этого — ошибка клиента, а не метрика.
const BODY_LIMIT: usize = 4 * 1024;
const TEXT_MAX: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Platform {
    Ios,
    Android,
}

/// Что разбудило приложение.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Trigger {
    SignificantChange,
    Visit,
    Continuous,
    Timer,
    Motion,
    Foreground,
    Boot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AppState {
    Foreground,
    Background,
    Relaunched,
}

/// Скорость корзинами, а не числом — нам нужна только «стоит / идёт / едет».
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SpeedBucket {
    Unknown,
    Still,
    Walking,
    Driving,
}

/// Одна метрика с устройства. Координат здесь нет и быть не может.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    /// Случайный ID установки, не связан с аккаунтом.
    device: String,
    platform: Platform,
    /// Стратегия из задач 1.2/1.3, например `s3` или `a2`.
    strategy: String,
    /// Unix-время события на устройстве, секунды.
    event_ts: i64,
    trigger: Trigger,
    app_state: AppState,
    /// Точность замера в метрах (сам замер не передаётся).
    accuracy_m: Option<u32>,
    speed: SpeedBucket,
    battery_pct: u8,
    charging: bool,
    low_power: bool,
    /// Задержка предыдущей отправки, мс; `None` — первая или не удалась.
    prev_send_ms: Option<u32>,
    /// Сколько предыдущих отправок не удалось.
    prev_send_failures: u32,
}

impl Probe {
    fn validate(&self) -> Result<(), &'static str> {
        let short = |s: &str| {
            !s.is_empty() && s.len() <= TEXT_MAX && s.bytes().all(|b| b.is_ascii_graphic())
        };
        if !short(&self.device) || !short(&self.strategy) {
            return Err("device and strategy must be short ASCII");
        }
        if self.battery_pct > 100 {
            return Err("battery_pct out of range");
        }
        Ok(())
    }
}

/// Строка в файле: метрика и время приёма сервером.
#[derive(Serialize)]
struct Stored<'a> {
    received_ts: i64,
    #[serde(flatten)]
    probe: &'a Probe,
}

struct AppStateInner {
    token: String,
    data_dir: PathBuf,
    /// Записи в файл по одной, чтобы строки JSONL не перемешивались.
    write_lock: Mutex<()>,
}

type Shared = Arc<AppStateInner>;

fn app(state: Shared) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/probe", post(probe))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

async fn probe(State(state): State<Shared>, headers: HeaderMap, body: Bytes) -> StatusCode {
    if !authorized(&headers, &state.token) {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(probe) = serde_json::from_slice::<Probe>(&body) else {
        return StatusCode::UNPROCESSABLE_ENTITY;
    };
    if probe.validate().is_err() {
        return StatusCode::UNPROCESSABLE_ENTITY;
    }
    let now = unix_now();
    let Ok(mut line) = serde_json::to_vec(&Stored {
        received_ts: now,
        probe: &probe,
    }) else {
        return StatusCode::INTERNAL_SERVER_ERROR;
    };
    line.push(b'\n');
    match append(&state, now, &line).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Сравнение без раннего выхода, чтобы время ответа не подсказывало токен.
fn authorized(headers: &HeaderMap, token: &str) -> bool {
    let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(given) = value.strip_prefix("Bearer ") else {
        return false;
    };
    let (a, b) = (given.as_bytes(), token.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn append(state: &AppStateInner, now: i64, line: &[u8]) -> std::io::Result<()> {
    let day = now.div_euclid(86_400);
    let path = state.data_dir.join(format!("probe-day{day}.jsonl"));
    let _guard = state.write_lock.lock().await;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line).await
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[tokio::main]
async fn main() {
    let token = std::env::var("PROBE_TOKEN").unwrap_or_default();
    // Короткий токен — ошибка конфигурации, а не повод работать без защиты.
    assert!(
        token.len() >= 32,
        "PROBE_TOKEN must be at least 32 characters"
    );
    let data_dir =
        PathBuf::from(std::env::var("PROBE_DATA_DIR").unwrap_or_else(|_| "/data".into()));
    let addr: SocketAddr = std::env::var("PROBE_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .expect("PROBE_LISTEN must be host:port");

    let state = Arc::new(AppStateInner {
        token,
        data_dir,
        write_lock: Mutex::new(()),
    });
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    eprintln!("probe-server listening on {addr}");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("serve");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn sample() -> serde_json::Value {
        serde_json::json!({
            "device": "dev-1", "platform": "ios", "strategy": "s3", "event_ts": 1_700_000_000,
            "trigger": "continuous", "app_state": "background", "accuracy_m": 12, "speed": "walking",
            "battery_pct": 87, "charging": false, "low_power": false,
            "prev_send_ms": 420, "prev_send_failures": 0
        })
    }

    fn setup() -> (tempfile::TempDir, Router) {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(AppStateInner {
            token: TOKEN.into(),
            data_dir: dir.path().to_path_buf(),
            write_lock: Mutex::new(()),
        });
        (dir, app(state))
    }

    async fn post(router: &Router, token: Option<&str>, body: Vec<u8>) -> StatusCode {
        let mut req = Request::post("/probe").header("content-type", "application/json");
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        router
            .clone()
            .oneshot(req.body(Body::from(body)).unwrap())
            .await
            .unwrap()
            .status()
    }

    fn stored(dir: &tempfile::TempDir) -> String {
        std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn stores_a_valid_probe() {
        let (dir, router) = setup();
        let status = post(&router, Some(TOKEN), serde_json::to_vec(&sample()).unwrap()).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let text = stored(&dir);
        assert_eq!(text.lines().count(), 1);
        let line: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(line["strategy"], "s3");
        assert!(line["received_ts"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn rejects_coordinates_and_unknown_fields() {
        let (dir, router) = setup();
        for field in ["lat", "lon", "latitude", "coordinates"] {
            let mut body = sample();
            body[field] = serde_json::json!(55.75);
            assert_eq!(
                post(&router, Some(TOKEN), serde_json::to_vec(&body).unwrap()).await,
                StatusCode::UNPROCESSABLE_ENTITY
            );
        }
        assert!(stored(&dir).is_empty());
    }

    #[tokio::test]
    async fn rejects_missing_or_wrong_token() {
        let (dir, router) = setup();
        let body = serde_json::to_vec(&sample()).unwrap();
        assert_eq!(
            post(&router, None, body.clone()).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            post(&router, Some("wrong"), body.clone()).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            post(&router, Some(&TOKEN[..31]), body).await,
            StatusCode::UNAUTHORIZED
        );
        assert!(stored(&dir).is_empty());
    }

    #[tokio::test]
    async fn rejects_oversized_and_invalid_values() {
        let (_dir, router) = setup();
        let mut big = sample();
        big["device"] = serde_json::json!("x".repeat(BODY_LIMIT));
        assert_eq!(
            post(&router, Some(TOKEN), serde_json::to_vec(&big).unwrap()).await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let mut bad = sample();
        bad["battery_pct"] = serde_json::json!(150);
        assert_eq!(
            post(&router, Some(TOKEN), serde_json::to_vec(&bad).unwrap()).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut long = sample();
        long["strategy"] = serde_json::json!("y".repeat(TEXT_MAX + 1));
        assert_eq!(
            post(&router, Some(TOKEN), serde_json::to_vec(&long).unwrap()).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            post(&router, Some(TOKEN), b"not json".to_vec()).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn ignores_proxy_headers() {
        // X-Forwarded-For от Caddy не попадает в запись.
        let (dir, router) = setup();
        let req = Request::post("/probe")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {TOKEN}"))
            .header("x-forwarded-for", "203.0.113.7")
            .body(Body::from(serde_json::to_vec(&sample()).unwrap()))
            .unwrap();
        assert_eq!(
            router.oneshot(req).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
        assert!(!stored(&dir).contains("203.0.113.7"));
    }
}
