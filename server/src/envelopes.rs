//! Почтовые ящики в PostgreSQL (protocol §8.1–8.3): те же правила, что у
//! dev-сервера в памяти (`mailbox.rs`), — размеры, лимит очереди, upsert слота,
//! статус каждого конверта.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use staya_proto::AccountId;
use staya_proto::api::{
    AckRequest, B64, EnvelopeKind, EnvelopeStatus, LocationSlot, MailboxResponse, QueuedControl,
    SendRequest, SendResponse,
};
use staya_proto::consts::{CONTROL_QUEUE_LIMIT, CONTROL_QUEUE_TTL, LOCATION_SLOT_TTL};

use crate::auth::Session;
use crate::http::{AppState, JsonBody, internal};
use crate::mailbox::{QUEUE_FULL, UNKNOWN_ACCOUNT, rejected, size_ok};

/// Конвертов в одном запросе: по одному на друга и вид с большим запасом.
pub const MAX_ENVELOPES_PER_REQUEST: usize = 256;
/// Номеров в одном ack.
pub const MAX_ACK: usize = 1000;

fn account(raw: Vec<u8>) -> Result<AccountId, StatusCode> {
    raw.try_into()
        .map(AccountId)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// `POST /v1/envelopes`. Одна транзакция на запрос; статус у каждого конверта.
pub async fn send(
    State(state): State<AppState>,
    Session(me): Session,
    JsonBody(req): JsonBody<SendRequest>,
) -> Result<Json<SendResponse>, StatusCode> {
    if req.envelopes.len() > MAX_ENVELOPES_PER_REQUEST {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let mut client = state.pool.get().await.map_err(internal)?;
    let tx = client.transaction().await.map_err(internal)?;
    // Все получатели запроса блокируются сразу и в одном порядке (ORDER BY):
    // параллельные отправители не превысят лимит очереди и не заблокируют друг друга.
    let recipients: Vec<&[u8]> = req.envelopes.iter().map(|e| e.to.0.as_slice()).collect();
    let known: std::collections::HashSet<Vec<u8>> = tx
        .query(
            "SELECT account_id FROM accounts WHERE account_id = ANY($1)
             ORDER BY account_id FOR UPDATE",
            &[&recipients],
        )
        .await
        .map_err(internal)?
        .into_iter()
        .map(|r| r.get(0))
        .collect();
    let mut results = Vec::with_capacity(req.envelopes.len());
    for env in &req.envelopes {
        if !size_ok(env.kind, env.data.0.len()) {
            results.push(rejected(crate::mailbox::BAD_SIZE));
            continue;
        }
        let to = env.to.0.as_slice();
        if !known.contains(to) {
            results.push(rejected(UNKNOWN_ACCOUNT));
            continue;
        }
        match env.kind {
            EnvelopeKind::Location => {
                tx.execute(
                    "INSERT INTO location_slots (sender, recipient, data) VALUES ($1, $2, $3)
                     ON CONFLICT (sender, recipient)
                     DO UPDATE SET data = EXCLUDED.data, updated_at = now()",
                    &[&me.0.as_slice(), &to, &env.data.0],
                )
                .await
                .map_err(internal)?;
            }
            EnvelopeKind::Control => {
                let queued: i64 = tx
                    .query_one(
                        "SELECT count(*) FROM control_queue WHERE recipient = $1",
                        &[&to],
                    )
                    .await
                    .map_err(internal)?
                    .get(0);
                if queued >= CONTROL_QUEUE_LIMIT as i64 {
                    results.push(rejected(QUEUE_FULL));
                    continue;
                }
                tx.execute(
                    "INSERT INTO control_queue (recipient, sender, data) VALUES ($1, $2, $3)",
                    &[&to, &me.0.as_slice(), &env.data.0],
                )
                .await
                .map_err(internal)?;
            }
        }
        results.push(EnvelopeStatus::Accepted);
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(SendResponse { results }))
}

/// `GET /v1/mailbox`: вся очередь по порядку и все свежие слоты для меня.
pub async fn mailbox(
    State(state): State<AppState>,
    Session(me): Session,
) -> Result<Json<MailboxResponse>, StatusCode> {
    let client = state.pool.get().await.map_err(internal)?;
    let control_ttl = CONTROL_QUEUE_TTL.as_secs_f64();
    let slot_ttl = LOCATION_SLOT_TTL.as_secs_f64();
    let control = client
        .query(
            "SELECT seq, sender, data FROM control_queue
             WHERE recipient = $1 AND created_at > now() - make_interval(secs => $2)
             ORDER BY seq",
            &[&me.0.as_slice(), &control_ttl],
        )
        .await
        .map_err(internal)?
        .into_iter()
        .map(|r| {
            Ok(QueuedControl {
                seq: r.get(0),
                from: account(r.get(1))?,
                data: B64(r.get(2)),
            })
        })
        .collect::<Result<Vec<_>, StatusCode>>()?;
    let locations = client
        .query(
            "SELECT sender, data FROM location_slots
             WHERE recipient = $1 AND updated_at > now() - make_interval(secs => $2)",
            &[&me.0.as_slice(), &slot_ttl],
        )
        .await
        .map_err(internal)?
        .into_iter()
        .map(|r| {
            Ok(LocationSlot {
                from: account(r.get(0))?,
                data: B64(r.get(1)),
            })
        })
        .collect::<Result<Vec<_>, StatusCode>>()?;
    Ok(Json(MailboxResponse { control, locations }))
}

/// `POST /v1/mailbox/ack`. Удаляет только из своей очереди.
pub async fn ack(
    State(state): State<AppState>,
    Session(me): Session,
    JsonBody(req): JsonBody<AckRequest>,
) -> StatusCode {
    if req.seqs.len() > MAX_ACK {
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    let result = async {
        let client = state.pool.get().await.map_err(internal)?;
        client
            .execute(
                "DELETE FROM control_queue WHERE recipient = $1 AND seq = ANY($2)",
                &[&me.0.as_slice(), &req.seqs],
            )
            .await
            .map_err(internal)?;
        Ok::<_, StatusCode>(StatusCode::NO_CONTENT)
    }
    .await;
    result.unwrap_or_else(|s| s)
}

/// `DELETE /v1/slots/{recipient}`: мой слот у получателя. Идемпотентно (§8.3).
pub async fn delete_slot(
    State(state): State<AppState>,
    Session(me): Session,
    Path(recipient): Path<String>,
) -> StatusCode {
    let Ok(to) = AccountId::from_b64(&recipient) else {
        return StatusCode::BAD_REQUEST;
    };
    let result = async {
        let client = state.pool.get().await.map_err(internal)?;
        client
            .execute(
                "DELETE FROM location_slots WHERE sender = $1 AND recipient = $2",
                &[&me.0.as_slice(), &to.0.as_slice()],
            )
            .await
            .map_err(internal)?;
        Ok::<_, StatusCode>(StatusCode::NO_CONTENT)
    }
    .await;
    result.unwrap_or_else(|s| s)
}

/// Удаляет управляющие сообщения старше 30 дней и слоты старше 72 часов.
pub async fn purge_expired(pool: &deadpool_postgres::Pool) -> Result<u64, crate::db::DbError> {
    let client = pool.get().await?;
    let a = client
        .execute(
            "DELETE FROM control_queue WHERE created_at <= now() - make_interval(secs => $1)",
            &[&CONTROL_QUEUE_TTL.as_secs_f64()],
        )
        .await?;
    let b = client
        .execute(
            "DELETE FROM location_slots WHERE updated_at <= now() - make_interval(secs => $1)",
            &[&LOCATION_SLOT_TTL.as_secs_f64()],
        )
        .await?;
    Ok(a + b)
}
