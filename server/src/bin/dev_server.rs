//! Dev-сервер Staya (задача 2.10): `cargo run -p staya-server --features dev --bin staya-dev-server`.
//! Адрес — `STAYA_DEV_LISTEN` (по умолчанию 127.0.0.1:8787); только loopback.
//! Эмулятор Android видит этот адрес как 10.0.2.2, симулятор iOS — как есть.

use std::net::SocketAddr;

use staya_server::dev::{DevState, app};

#[tokio::main]
async fn main() {
    let addr: SocketAddr = std::env::var("STAYA_DEV_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()
        .expect("STAYA_DEV_LISTEN: host:port");
    assert!(
        addr.ip().is_loopback(),
        "dev server has no authentication: loopback only"
    );
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    eprintln!("staya-dev-server on http://{addr}");
    axum::serve(listener, app(DevState::default()))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("serve");
}
