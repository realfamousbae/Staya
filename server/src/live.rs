//! Живая доставка по WebSocket (задача 3.5, protocol §8.2.6).
//!
//! Пока приложение открыто, оно держит `GET /v1/ws` и получает новые конверты
//! сразу после записи в базу. Это только ускорение: всё доставленное остаётся в
//! ящике, управляющие сообщения подтверждаются как обычно (`/v1/mailbox/ack`).
//! Один экземпляр сервера — подписки в памяти процесса.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use staya_proto::AccountId;
use staya_proto::api::WsEvent;
use tokio::sync::mpsc;

use crate::auth::Session;
use crate::http::AppState;

/// Одновременных соединений на аккаунт (телефон, переподключение внахлёст).
pub const MAX_CONNECTIONS_PER_ACCOUNT: usize = 4;
/// Событий в очереди одного соединения; медленный клиент теряет лишнее — оно
/// всё равно лежит в ящике.
const QUEUE: usize = 64;
/// Ping, чтобы NAT и прокси не закрыли тихое соединение и мёртвое обнаружилось.
const PING_EVERY: Duration = Duration::from_secs(30);
/// Соединение живёт не дольше: потом клиент переподключается с текущим токеном.
const MAX_LIFETIME: Duration = Duration::from_secs(3600);

type Subscribers = HashMap<AccountId, Vec<(u64, mpsc::Sender<WsEvent>)>>;

#[derive(Default)]
pub struct Hub {
    subscribers: Mutex<Subscribers>,
    next: AtomicU64,
}

impl Hub {
    fn lock(&self) -> std::sync::MutexGuard<'_, Subscribers> {
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `None` — у аккаунта уже максимум соединений.
    pub fn subscribe(&self, who: AccountId) -> Option<(u64, mpsc::Receiver<WsEvent>)> {
        let mut subs = self.lock();
        let list = subs.entry(who).or_default();
        list.retain(|(_, tx)| !tx.is_closed());
        if list.len() >= MAX_CONNECTIONS_PER_ACCOUNT {
            return None;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(QUEUE);
        list.push((id, tx));
        Some((id, rx))
    }

    pub fn unsubscribe(&self, who: &AccountId, id: u64) {
        let mut subs = self.lock();
        if let Some(list) = subs.get_mut(who) {
            list.retain(|(n, _)| *n != id);
            if list.is_empty() {
                subs.remove(who);
            }
        }
    }

    /// Не ждёт: если очередь соединения полна, событие пропускается.
    pub fn notify(&self, who: &AccountId, event: &WsEvent) {
        if let Some(list) = self.lock().get(who) {
            for (_, tx) in list {
                let _ = tx.try_send(event.clone());
            }
        }
    }

    pub fn connections(&self, who: &AccountId) -> usize {
        self.lock().get(who).map_or(0, Vec::len)
    }
}

/// `GET /v1/ws` с `Authorization: Bearer <токен>`. 429 — слишком много соединений.
pub async fn ws(
    State(state): State<AppState>,
    Session(me): Session,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some((id, rx)) = state.hub.subscribe(me) else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let hub = state.hub.clone();
    upgrade
        // Клиенту нечего присылать, кроме служебных кадров.
        .max_message_size(1024)
        .on_failed_upgrade({
            let hub = hub.clone();
            move |_| hub.unsubscribe(&me, id)
        })
        .on_upgrade(move |socket| async move {
            serve(socket, rx).await;
            hub.unsubscribe(&me, id);
        })
}

async fn serve(mut socket: WebSocket, mut rx: mpsc::Receiver<WsEvent>) {
    let deadline = tokio::time::sleep(MAX_LIFETIME);
    tokio::pin!(deadline);
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else { break };
                let Ok(text) = serde_json::to_string(&event) else { continue };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            incoming = socket.recv() => match incoming {
                // Pong и прочее служебное — игнорируем; данные от клиента не ждём.
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            () = &mut deadline => {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
        }
    }
}
