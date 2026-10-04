//! Каталог ключей: публикация, выдача и счётчик (protocol §4.3, §8.3).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use staya_proto::api::{
    B64, ClaimRequest, ClaimResponse, KeyCountResponse, PublishKeysRequest, SignedKey,
};
use staya_proto::signing;

use crate::auth::{Session, verify};
use crate::http::{AppState, JsonBody, internal};

/// Сколько OTK аккаунт может держать на сервере: клиент держит 50 (§3), запас —
/// на повтор публикации после сбоя.
pub const MAX_ONE_TIME_KEYS: i64 = 100;

fn signed(k: &SignedKey) -> Option<([u8; 32], [u8; 64])> {
    Some((
        k.key.0.as_slice().try_into().ok()?,
        k.signature.0.as_slice().try_into().ok()?,
    ))
}

/// `PUT /v1/keys`. Все подписи проверяются ключом `sk` аккаунта; одна неверная —
/// 400 для всего запроса. Повтор уже опубликованного OTK ничего не меняет.
pub async fn publish(
    State(state): State<AppState>,
    Session(me): Session,
    JsonBody(req): JsonBody<PublishKeysRequest>,
) -> StatusCode {
    let result = async {
        let mut client = state.pool.get().await.map_err(internal)?;
        let sk: Vec<u8> = client
            .query_one(
                "SELECT sk FROM accounts WHERE account_id = $1",
                &[&me.0.as_slice()],
            )
            .await
            .map_err(internal)?
            .get(0);
        let mut otks = Vec::with_capacity(req.one_time_keys.len());
        for k in &req.one_time_keys {
            let (key, sig) = signed(k).ok_or(StatusCode::BAD_REQUEST)?;
            if !verify(&sk, &signing::one_time_key(&key), &sig) {
                return Err(StatusCode::BAD_REQUEST);
            }
            otks.push((key, sig));
        }
        let fallback = match &req.fallback_key {
            Some(k) => {
                let (key, sig) = signed(k).ok_or(StatusCode::BAD_REQUEST)?;
                if !verify(&sk, &signing::fallback_key(&key), &sig) {
                    return Err(StatusCode::BAD_REQUEST);
                }
                Some((key, sig))
            }
            None => None,
        };

        let tx = client.transaction().await.map_err(internal)?;
        // Публикации одного аккаунта по очереди: иначе две параллельные обошли бы лимит.
        tx.execute(
            "SELECT 1 FROM accounts WHERE account_id = $1 FOR UPDATE",
            &[&me.0.as_slice()],
        )
        .await
        .map_err(internal)?;
        for (key, sig) in &otks {
            tx.execute(
                "INSERT INTO one_time_keys (account_id, key, signature) VALUES ($1, $2, $3)
                 ON CONFLICT (account_id, key) DO NOTHING",
                &[&me.0.as_slice(), &key.as_slice(), &sig.as_slice()],
            )
            .await
            .map_err(internal)?;
        }
        let count: i64 = tx
            .query_one(
                "SELECT count(*) FROM one_time_keys WHERE account_id = $1",
                &[&me.0.as_slice()],
            )
            .await
            .map_err(internal)?
            .get(0);
        if count > MAX_ONE_TIME_KEYS {
            // Транзакция откатывается при выходе.
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        if let Some((key, sig)) = fallback {
            tx.execute(
                "INSERT INTO fallback_keys (account_id, key, signature) VALUES ($1, $2, $3)
                 ON CONFLICT (account_id) DO UPDATE SET key = $2, signature = $3",
                &[&me.0.as_slice(), &key.as_slice(), &sig.as_slice()],
            )
            .await
            .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(StatusCode::NO_CONTENT)
    }
    .await;
    result.unwrap_or_else(|s| s)
}

/// `POST /v1/keys/claim`: самый старый OTK аккаунта (удаляется), а если их нет —
/// fallback-ключ. 404 — у аккаунта нет ключей или его нет совсем.
pub async fn claim(
    State(state): State<AppState>,
    Session(_): Session,
    JsonBody(req): JsonBody<ClaimRequest>,
) -> Result<Json<ClaimResponse>, StatusCode> {
    let client = state.pool.get().await.map_err(internal)?;
    let target = req.account_id.0.as_slice();
    // SKIP LOCKED: параллельные claim получают разные ключи, не дожидаясь друг друга.
    let otk = client
        .query_opt(
            "DELETE FROM one_time_keys WHERE id = (
                 SELECT id FROM one_time_keys WHERE account_id = $1
                 ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED
             ) RETURNING key, signature",
            &[&target],
        )
        .await
        .map_err(internal)?;
    let (row, is_fallback) = match otk {
        Some(row) => (row, false),
        None => (
            client
                .query_opt(
                    "SELECT key, signature FROM fallback_keys WHERE account_id = $1",
                    &[&target],
                )
                .await
                .map_err(internal)?
                .ok_or(StatusCode::NOT_FOUND)?,
            true,
        ),
    };
    Ok(Json(ClaimResponse {
        key: SignedKey {
            key: B64(row.get(0)),
            signature: B64(row.get(1)),
        },
        is_fallback,
    }))
}

/// `GET /v1/keys/count`: сколько своих OTK осталось на сервере.
pub async fn count(
    State(state): State<AppState>,
    Session(me): Session,
) -> Result<Json<KeyCountResponse>, StatusCode> {
    let client = state.pool.get().await.map_err(internal)?;
    let n: i64 = client
        .query_one(
            "SELECT count(*) FROM one_time_keys WHERE account_id = $1",
            &[&me.0.as_slice()],
        )
        .await
        .map_err(internal)?
        .get(0);
    Ok(Json(KeyCountResponse {
        one_time_keys: u32::try_from(n).unwrap_or(u32::MAX),
    }))
}
