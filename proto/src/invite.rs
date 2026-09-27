//! Приглашение в друзья: ссылка и QR (§5).
//!
//! `staya://add?v=1&id=<account_id>&ik=<ik>&sk=<sk>&t=<token>&m=<q|l>`,
//! все значения — base64url без выравнивания.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::{AccountId, ProtoError};

const PREFIX: &str = "staya://add?";
const VERSION: &str = "1";

/// Как приглашение было передано — от этого зависит статус проверки (§5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InviteMethod {
    /// QR при встрече: ключи пришли по физическому каналу.
    Qr,
    /// Ссылка: нужен код безопасности.
    Link,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Invite {
    pub account_id: AccountId,
    pub ik: [u8; 32],
    pub sk: [u8; 32],
    pub token: [u8; 16],
    pub method: InviteMethod,
}

impl fmt::Debug for Invite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Токен — секрет приглашения.
        f.debug_struct("Invite")
            .field("account_id", &self.account_id)
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl Invite {
    pub fn to_uri(&self) -> String {
        let m = match self.method {
            InviteMethod::Qr => "q",
            InviteMethod::Link => "l",
        };
        format!(
            "{PREFIX}v={VERSION}&id={}&ik={}&sk={}&t={}&m={m}",
            self.account_id.to_b64(),
            URL_SAFE_NO_PAD.encode(self.ik),
            URL_SAFE_NO_PAD.encode(self.sk),
            URL_SAFE_NO_PAD.encode(self.token),
        )
    }

    pub fn parse(uri: &str) -> Result<Self, ProtoError> {
        let query = uri
            .strip_prefix(PREFIX)
            .ok_or(ProtoError::Invalid("invite scheme"))?;
        let (mut v, mut id, mut ik, mut sk, mut t, mut m) = (None, None, None, None, None, None);
        for pair in query.split('&') {
            let (key, value) = pair
                .split_once('=')
                .ok_or(ProtoError::Invalid("invite query"))?;
            let slot = match key {
                "v" => &mut v,
                "id" => &mut id,
                "ik" => &mut ik,
                "sk" => &mut sk,
                "t" => &mut t,
                "m" => &mut m,
                _ => continue, // неизвестные параметры игнорируются
            };
            if slot.replace(value).is_some() {
                return Err(ProtoError::Invalid("duplicate invite parameter"));
            }
        }
        if v != Some(VERSION) {
            return Err(ProtoError::Invalid("invite version"));
        }
        let method = match m {
            Some("q") => InviteMethod::Qr,
            Some("l") => InviteMethod::Link,
            _ => return Err(ProtoError::Invalid("invite method")),
        };
        Ok(Self {
            account_id: AccountId::from_b64(id.ok_or(ProtoError::Invalid("invite id"))?)?,
            ik: decode_array(ik, "invite ik")?,
            sk: decode_array(sk, "invite sk")?,
            token: decode_array(t, "invite token")?,
            method,
        })
    }
}

fn decode_array<const N: usize>(
    value: Option<&str>,
    what: &'static str,
) -> Result<[u8; N], ProtoError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value.ok_or(ProtoError::Invalid(what))?)
        .map_err(|_| ProtoError::Invalid(what))?;
    bytes.try_into().map_err(|_| ProtoError::Invalid(what))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn roundtrip(id: [u8; 16], ik: [u8; 32], sk: [u8; 32], token: [u8; 16], qr: bool) {
            let inv = Invite {
                account_id: AccountId(id),
                ik,
                sk,
                token,
                method: if qr { InviteMethod::Qr } else { InviteMethod::Link },
            };
            prop_assert_eq!(Invite::parse(&inv.to_uri()).unwrap(), inv);
        }

        #[test]
        fn parse_never_panics(s in "\\PC{0,200}") {
            let _ = Invite::parse(&s);
            let _ = Invite::parse(&format!("staya://add?{s}"));
        }
    }

    #[test]
    fn uri_fits_in_a_qr_code() {
        let inv = Invite {
            account_id: AccountId([0; 16]),
            ik: [0; 32],
            sk: [0; 32],
            token: [0; 16],
            method: InviteMethod::Qr,
        };
        // QR версии 7 с коррекцией M вмещает 122 байта; с запасом держимся до 200.
        assert!(inv.to_uri().len() < 200, "{}", inv.to_uri().len());
    }

    #[test]
    fn rejects_duplicates_and_bad_values() {
        let inv = Invite {
            account_id: AccountId([1; 16]),
            ik: [2; 32],
            sk: [3; 32],
            token: [4; 16],
            method: InviteMethod::Link,
        };
        let uri = inv.to_uri();
        assert!(Invite::parse(&format!("{uri}&m=q")).is_err());
        assert!(Invite::parse(&uri.replace("v=1", "v=2")).is_err());
        assert!(Invite::parse(&uri.replace("staya://", "https://")).is_err());
    }

    #[test]
    fn debug_hides_token() {
        let inv = Invite {
            account_id: AccountId([1; 16]),
            ik: [2; 32],
            sk: [3; 32],
            token: [0xEE; 16],
            method: InviteMethod::Qr,
        };
        assert!(!format!("{inv:?}").contains("238"));
    }
}
