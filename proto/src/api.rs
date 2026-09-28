//! JSON-схемы HTTP API (§8.3). Двоичные поля — base64 (стандартный алфавит).

use serde::{Deserialize, Serialize};

use crate::AccountId;

/// Двоичные данные, в JSON — строка base64.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct B64(#[serde(with = "b64")] pub Vec<u8>);

impl std::fmt::Debug for B64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "B64({} bytes)", self.0.len())
    }
}

mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

/// `POST /v1/accounts`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub account_id: AccountId,
    pub ik: B64,
    pub sk: B64,
    /// Подпись `signing::register`.
    pub signature: B64,
    /// Код приглашения на сервер (только в бете).
    pub invite_code: Option<String>,
}

/// `POST /v1/auth/challenge`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeRequest {
    pub account_id: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChallengeResponse {
    pub nonce: B64,
}

/// `POST /v1/auth/verify`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyRequest {
    pub account_id: AccountId,
    pub nonce: B64,
    /// Подпись `signing::auth`.
    pub signature: B64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub token: B64,
    /// Unix-время окончания действия токена, секунды.
    pub expires_at: i64,
}

/// Публичный ключ с подписью ключом `sk`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedKey {
    pub key: B64,
    pub signature: B64,
}

/// `PUT /v1/keys`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishKeysRequest {
    pub one_time_keys: Vec<SignedKey>,
    pub fallback_key: Option<SignedKey>,
}

/// `POST /v1/keys/claim`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRequest {
    pub account_id: AccountId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimResponse {
    pub key: SignedKey,
    /// `true`, если OTK закончились и выдан fallback-ключ.
    pub is_fallback: bool,
}

/// `GET /v1/keys/count`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyCountResponse {
    pub one_time_keys: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKind {
    Location,
    Control,
}

/// Исходящий конверт. Отправителя сервер берёт из токена.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutgoingEnvelope {
    pub to: AccountId,
    pub kind: EnvelopeKind,
    pub data: B64,
}

/// `POST /v1/envelopes`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRequest {
    pub envelopes: Vec<OutgoingEnvelope>,
}

/// Статус одного конверта в ответе на `POST /v1/envelopes`, в том же порядке.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EnvelopeStatus {
    Accepted,
    /// Окончательно: повтор не поможет (очередь получателя полна, неверный размер,
    /// неизвестный аккаунт). Клиент списывает конверт из исходящей очереди.
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendResponse {
    pub results: Vec<EnvelopeStatus>,
}

/// Управляющее сообщение из очереди.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedControl {
    /// Порядковый номер для `ack`.
    pub seq: i64,
    /// Подсказка сервера; доверять нельзя (§7.5).
    pub from: AccountId,
    pub data: B64,
}

/// Последний конверт позиции от отправителя.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationSlot {
    /// Подсказка сервера; доверять нельзя (§7.5).
    pub from: AccountId,
    pub data: B64,
}

/// `GET /v1/mailbox`. Клиент обрабатывает сначала `control`, затем `locations` (§8.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxResponse {
    pub control: Vec<QueuedControl>,
    pub locations: Vec<LocationSlot>,
}

/// `POST /v1/mailbox/ack`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AckRequest {
    pub seqs: Vec<i64>,
}

/// Событие WebSocket `GET /v1/ws`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    Control(QueuedControl),
    Location(LocationSlot),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_json_shape() {
        let env = OutgoingEnvelope {
            to: AccountId([0; 16]),
            kind: EnvelopeKind::Location,
            data: B64(vec![1, 2, 3]),
        };
        let json = serde_json::to_string(&env).unwrap();
        assert_eq!(
            json,
            r#"{"to":"AAAAAAAAAAAAAAAAAAAAAA","kind":"location","data":"AQID"}"#
        );
        assert_eq!(
            serde_json::from_str::<OutgoingEnvelope>(&json).unwrap(),
            env
        );
    }

    #[test]
    fn ws_event_is_tagged() {
        let ev = WsEvent::Location(LocationSlot {
            from: AccountId([0; 16]),
            data: B64(vec![]),
        });
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.starts_with(r#"{"type":"location""#), "{json}");
        assert_eq!(serde_json::from_str::<WsEvent>(&json).unwrap(), ev);
    }

    #[test]
    fn rejects_bad_base64() {
        assert!(serde_json::from_str::<B64>(r#""not base64!""#).is_err());
    }
}
