//! Сервер Staya. Настройки — переменные окружения:
//! - `STAYA_DATABASE_URL` — обязательно, `postgres://user:password@host:port/db`;
//! - `STAYA_LISTEN` — адрес, по умолчанию `127.0.0.1:8080`;
//! - `STAYA_LOG` — `error` | `warn` | `info` (по умолчанию) | `debug`.

use std::net::SocketAddr;
use std::process::ExitCode;

use staya_server::db;
use staya_server::http::{AppState, app};
use tracing_subscriber::filter::LevelFilter;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() -> ExitCode {
    let level: LevelFilter = env("STAYA_LOG")
        .and_then(|l| l.parse().ok())
        .unwrap_or(LevelFilter::INFO);
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .init();

    let Some(url) = env("STAYA_DATABASE_URL") else {
        tracing::error!("STAYA_DATABASE_URL is not set");
        return ExitCode::FAILURE;
    };
    let addr: SocketAddr = match env("STAYA_LISTEN")
        .unwrap_or_else(|| "127.0.0.1:8080".into())
        .parse()
    {
        Ok(a) => a,
        Err(_) => {
            tracing::error!("STAYA_LISTEN must be host:port");
            return ExitCode::FAILURE;
        }
    };

    let pool = match db::pool(&url, 16) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match db::migrate(&pool, db::MIGRATIONS).await {
        Ok(version) => tracing::info!(version, "database schema ready"),
        Err(e) => {
            tracing::error!("{e}");
            return ExitCode::FAILURE;
        }
    }

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("bind {addr}: {e}");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(%addr, "listening");
    let served = axum::serve(listener, app(AppState { pool: pool.clone() }))
        .with_graceful_shutdown(shutdown_signal())
        .await;
    pool.close();
    match served {
        Ok(()) => {
            tracing::info!("stopped");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!("serve: {e}");
            ExitCode::FAILURE
        }
    }
}

/// SIGTERM (`docker stop`) или Ctrl-C.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}
