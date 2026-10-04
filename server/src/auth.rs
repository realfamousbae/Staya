//! Регистрация и вход (protocol §4.1, §4.2).
//!
//! Вход — подпись случайного одноразового nonce ключом `sk` с именем сервера внутри,
//! так что подпись для одного сервера не подходит другому. Токен сессии — 32
//! случайных байта; в базе только его SHA-256.

use axum::extract::{FromRef, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use staya_proto::AccountId;
use staya_proto::api::{
    B64, ChallengeRequest, ChallengeResponse, RegisterRequest, VerifyRequest, VerifyResponse,
};
use staya_proto::consts::{AUTH_CHALLENGE_TTL, AUTH_TOKEN_TTL};
use staya_proto::signing;
use subtle::ConstantTimeEq;

use crate::http::{AppState, JsonBody, internal};

fn verify(sk: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let (Ok(sk), Ok(sig)) = (<[u8; 32]>::try_from(sk), <[u8; 64]>::try_from(signature)) else {
        return false;
    };
    VerifyingKey::from_bytes(&sk).is_ok_and(|key| {
        key.verify_strict(message, &Signature::from_bytes(&sig))
            .is_ok()
    })
}

fn random32() -> Result<[u8; 32], StatusCode> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(b)
}

fn invite_ok(expected: Option<&str>, given: Option<&str>) -> bool {
    match (expected, given) {
        (None, _) => true,
        (Some(e), Some(g)) => e.as_bytes().ct_eq(g.as_bytes()).into(),
        (Some(_), None) => false,
    }
}

/// `POST /v1/accounts`. 201 — создан, 200 — уже есть с теми же ключами (повтор
/// после потери базы клиентом или сервером), 409 — ID занят другими ключами.
pub async fn register(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<RegisterRequest>,
) -> StatusCode {
    let (Ok(ik), Ok(sk)) = (
        <[u8; 32]>::try_from(req.ik.0.as_slice()),
        <[u8; 32]>::try_from(req.sk.0.as_slice()),
    ) else {
        return StatusCode::BAD_REQUEST;
    };
    if !verify(
        &sk,
        &signing::register(&req.account_id, &ik, &sk),
        &req.signature.0,
    ) {
        return StatusCode::BAD_REQUEST;
    }
    let result = async {
        let client = state.pool.get().await.map_err(internal)?;
        let existing = client
            .query_opt(
                "SELECT ik, sk FROM accounts WHERE account_id = $1",
                &[&req.account_id.0.as_slice()],
            )
            .await
            .map_err(internal)?;
        if let Some(row) = existing {
            let (old_ik, old_sk): (Vec<u8>, Vec<u8>) = (row.get(0), row.get(1));
            return Ok(if old_ik == ik && old_sk == sk {
                StatusCode::OK
            } else {
                StatusCode::CONFLICT
            });
        }
        if !invite_ok(
            state.config.invite_code.as_deref(),
            req.invite_code.as_deref(),
        ) {
            return Ok(StatusCode::FORBIDDEN);
        }
        let inserted = client
            .execute(
                "INSERT INTO accounts (account_id, ik, sk) VALUES ($1, $2, $3)
                 ON CONFLICT (account_id) DO NOTHING",
                &[&req.account_id.0.as_slice(), &ik.as_slice(), &sk.as_slice()],
            )
            .await
            .map_err(internal)?;
        // Гонка двух одинаковых регистраций: проигравший — как повтор.
        Ok::<_, StatusCode>(if inserted == 1 {
            StatusCode::CREATED
        } else {
            StatusCode::CONFLICT
        })
    }
    .await;
    result.unwrap_or_else(|s| s)
}

/// `POST /v1/auth/challenge`. 404 — аккаунта нет: клиент регистрируется заново.
pub async fn challenge(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<ChallengeRequest>,
) -> Result<axum::Json<ChallengeResponse>, StatusCode> {
    let nonce = random32()?;
    let client = state.pool.get().await.map_err(internal)?;
    let ttl = AUTH_CHALLENGE_TTL.as_secs_f64();
    let inserted = client
        .execute(
            "INSERT INTO auth_challenges (nonce, account_id, expires_at)
             SELECT $1, account_id, now() + make_interval(secs => $3)
             FROM accounts WHERE account_id = $2",
            &[&nonce.as_slice(), &req.account_id.0.as_slice(), &ttl],
        )
        .await
        .map_err(internal)?;
    if inserted == 0 {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(axum::Json(ChallengeResponse {
        nonce: B64(nonce.to_vec()),
    }))
}

/// `POST /v1/auth/verify`. Nonce сгорает при любой попытке; 401 — неверная подпись,
/// чужой, просроченный или уже использованный nonce.
pub async fn verify_challenge(
    State(state): State<AppState>,
    JsonBody(req): JsonBody<VerifyRequest>,
) -> Result<axum::Json<VerifyResponse>, StatusCode> {
    let Ok(nonce) = <[u8; 32]>::try_from(req.nonce.0.as_slice()) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let client = state.pool.get().await.map_err(internal)?;
    let row = client
        .query_opt(
            "DELETE FROM auth_challenges c USING accounts a
             WHERE c.nonce = $1 AND c.account_id = $2 AND c.expires_at > now()
               AND a.account_id = c.account_id
             RETURNING a.sk",
            &[&nonce.as_slice(), &req.account_id.0.as_slice()],
        )
        .await
        .map_err(internal)?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let sk: Vec<u8> = row.get(0);
    let message = signing::auth(&state.config.domain, &nonce, &req.account_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !verify(&sk, &message, &req.signature.0) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let token = random32()?;
    let hash = Sha256::digest(token);
    let ttl = AUTH_TOKEN_TTL.as_secs_f64();
    let expires_at: f64 = client
        .query_one(
            "INSERT INTO sessions (token_hash, account_id, expires_at)
             VALUES ($1, $2, now() + make_interval(secs => $3))
             RETURNING extract(epoch FROM expires_at)::float8",
            &[&hash.as_slice(), &req.account_id.0.as_slice(), &ttl],
        )
        .await
        .map_err(internal)?
        .get(0);
    Ok(axum::Json(VerifyResponse {
        token: B64(token.to_vec()),
        expires_at: expires_at as i64,
    }))
}

/// Аккаунт по токену сессии из `Authorization: Bearer <base64>`. 401 — нет,
/// неверный или просроченный токен.
pub struct Session(pub AccountId);

impl<S> FromRequestParts<S> for Session
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .and_then(|t| STANDARD.decode(t).ok())
            .filter(|t| t.len() == 32)
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let state = AppState::from_ref(state);
        let client = state.pool.get().await.map_err(internal)?;
        let row = client
            .query_opt(
                "SELECT account_id FROM sessions WHERE token_hash = $1 AND expires_at > now()",
                &[&Sha256::digest(&token).as_slice()],
            )
            .await
            .map_err(internal)?
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let id: Vec<u8> = row.get(0);
        let id: [u8; 16] = id.try_into().map_err(|_| StatusCode::UNAUTHORIZED)?;
        Ok(Session(AccountId(id)))
    }
}

/// Удаляет просроченные challenge и сессии. Возвращает число удалённых строк.
pub async fn purge_expired(pool: &deadpool_postgres::Pool) -> Result<u64, crate::db::DbError> {
    let client = pool.get().await?;
    let a = client
        .execute("DELETE FROM auth_challenges WHERE expires_at <= now()", &[])
        .await?;
    let b = client
        .execute("DELETE FROM sessions WHERE expires_at <= now()", &[])
        .await?;
    Ok(a + b)
}

#[cfg(test)]
mod tests {
    use super::invite_ok;

    #[test]
    fn invite_code_rules() {
        assert!(invite_ok(None, None));
        assert!(invite_ok(None, Some("x")));
        assert!(invite_ok(Some("beta"), Some("beta")));
        assert!(!invite_ok(Some("beta"), Some("BETA")));
        assert!(!invite_ok(Some("beta"), None));
    }
}
