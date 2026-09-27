//! Байты, которые подписываются ключом `sk` (§4).
//!
//! У каждого назначения своя строка-контекст с нулевым байтом в конце, поэтому
//! ни одна подписанная строка не может быть префиксом другой.

use crate::{AccountId, ProtoError};

const REGISTER: &[u8] = b"staya/v1/register\0";
const AUTH: &[u8] = b"staya/v1/auth\0";
const OTK: &[u8] = b"staya/v1/otk\0";
const FALLBACK: &[u8] = b"staya/v1/fallback\0";

pub fn register(account_id: &AccountId, ik: &[u8; 32], sk: &[u8; 32]) -> Vec<u8> {
    [REGISTER, &account_id.0, ik, sk].concat()
}

/// `domain` — хост сервера, ASCII, не длиннее 255 байт.
pub fn auth(domain: &str, nonce: &[u8; 32], account_id: &AccountId) -> Result<Vec<u8>, ProtoError> {
    if !domain.is_ascii() {
        return Err(ProtoError::Invalid("domain"));
    }
    let len = u8::try_from(domain.len()).map_err(|_| ProtoError::TooLarge("domain"))?;
    Ok([AUTH, &[len], domain.as_bytes(), nonce, &account_id.0].concat())
}

pub fn one_time_key(key: &[u8; 32]) -> Vec<u8> {
    [OTK, key].concat()
}

pub fn fallback_key(key: &[u8; 32]) -> Vec<u8> {
    [FALLBACK, key].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contexts_are_prefix_free() {
        let all = [REGISTER, AUTH, OTK, FALLBACK];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert!(!b.starts_with(a), "{a:?} is a prefix of {b:?}");
                }
            }
        }
    }

    #[test]
    fn otk_and_fallback_messages_differ() {
        let key = [3u8; 32];
        assert_ne!(one_time_key(&key), fallback_key(&key));
    }

    #[test]
    fn auth_layout() {
        let msg = auth("staya.example", &[1; 32], &AccountId([2; 16])).unwrap();
        assert_eq!(msg.len(), AUTH.len() + 1 + 13 + 32 + 16);
        assert!(auth("пример", &[0; 32], &AccountId([0; 16])).is_err());
    }
}
