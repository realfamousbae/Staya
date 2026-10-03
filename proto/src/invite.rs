//! Приглашение в друзья: ссылка и QR (§5).
//!
//! `staya://add?v=1&s=<server>&p=<pin>&id=<account_id>&ik=<ik>&sk=<sk>&t=<token>&m=<q|l>`;
//! `s` — сервер пригласившего (§5.3), `p` — необязательный отпечаток ключа TLS;
//! двоичные значения — base64url без выравнивания.

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

/// Сервер аккаунта (§5.3): хост с необязательным портом и отпечаток ключа TLS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerRef {
    /// `host` или `host:port`, только ASCII; порт 443 не пишется.
    pub host: String,
    /// SHA-256 от SubjectPublicKeyInfo ключа TLS сервера.
    pub pin: Option<[u8; 32]>,
}

impl ServerRef {
    pub fn new(host: &str, pin: Option<[u8; 32]>) -> Result<Self, ProtoError> {
        let host = host.trim().to_ascii_lowercase();
        Self::validate(&host)?;
        Ok(Self { host, pin })
    }

    /// Имя хоста по правилам DNS (буквы, цифры, `-`, `.`) или IPv4, плюс `:порт`.
    fn validate(host: &str) -> Result<(), ProtoError> {
        let (name, port) = match host.rsplit_once(':') {
            Some((n, p)) => (n, Some(p)),
            None => (host, None),
        };
        if let Some(p) = port {
            let ok = !p.is_empty()
                && p.len() <= 5
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n));
            if !ok {
                return Err(ProtoError::Invalid("server port"));
            }
        }
        let labels_ok = !name.is_empty()
            && name.len() <= 253
            && name.split('.').all(|l| {
                !l.is_empty()
                    && l.len() <= 63
                    && !l.starts_with('-')
                    && !l.ends_with('-')
                    && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
        if labels_ok {
            Ok(())
        } else {
            Err(ProtoError::Invalid("server host"))
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Invite {
    pub server: ServerRef,
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
            .field("server", &self.server.host)
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
        let pin = self
            .server
            .pin
            .map(|p| format!("&p={}", URL_SAFE_NO_PAD.encode(p)))
            .unwrap_or_default();
        format!(
            "{PREFIX}v={VERSION}&s={}{pin}&id={}&ik={}&sk={}&t={}&m={m}",
            self.server.host,
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
        let (mut v, mut srv, mut pin, mut id, mut ik, mut sk, mut t, mut m) =
            (None, None, None, None, None, None, None, None);
        for pair in query.split('&') {
            let (key, value) = pair
                .split_once('=')
                .ok_or(ProtoError::Invalid("invite query"))?;
            let slot = match key {
                "v" => &mut v,
                "s" => &mut srv,
                "p" => &mut pin,
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
        let pin = match pin {
            Some(_) => Some(decode_array(pin, "invite server pin")?),
            None => None,
        };
        let server = ServerRef::new(srv.ok_or(ProtoError::Invalid("invite server"))?, pin)?;
        Ok(Self {
            server,
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

    fn server(pin: Option<[u8; 32]>) -> ServerRef {
        ServerRef::new("staya.example.org", pin).unwrap()
    }

    fn invite(method: InviteMethod, pin: Option<[u8; 32]>) -> Invite {
        Invite {
            server: server(pin),
            account_id: AccountId([1; 16]),
            ik: [2; 32],
            sk: [3; 32],
            token: [4; 16],
            method,
        }
    }

    proptest! {
        #[test]
        fn roundtrip(
            id: [u8; 16], ik: [u8; 32], sk: [u8; 32], token: [u8; 16], qr: bool,
            pin in proptest::option::of(any::<[u8; 32]>()),
            host in "[a-z0-9]{1,20}(\\.[a-z0-9]{1,10}){0,3}(:[1-9][0-9]{0,3})?",
        ) {
            let inv = Invite {
                server: ServerRef::new(&host, pin).unwrap(),
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
        // QR в байтовом режиме с коррекцией M: версия 13 вмещает 331 байт и
        // ещё уверенно сканируется с экрана телефона.
        let uri = invite(InviteMethod::Qr, Some([9; 32])).to_uri();
        assert!(uri.len() <= 331, "{} bytes: {uri}", uri.len());
    }

    #[test]
    fn server_and_pin_are_carried() {
        let inv = Invite::parse(&invite(InviteMethod::Link, Some([7; 32])).to_uri()).unwrap();
        assert_eq!(inv.server.host, "staya.example.org");
        assert_eq!(inv.server.pin, Some([7; 32]));
        let no_pin = Invite::parse(&invite(InviteMethod::Link, None).to_uri()).unwrap();
        assert_eq!(no_pin.server.pin, None);
        assert!(!invite(InviteMethod::Link, None).to_uri().contains("&p="));
    }

    #[test]
    fn server_host_validation() {
        for ok in [
            "example.org",
            "a.b.c",
            "10.0.0.1",
            "host:8443",
            "2-27-x.sslip.io",
            "Example.ORG",
        ] {
            assert!(ServerRef::new(ok, None).is_ok(), "{ok}");
        }
        assert_eq!(
            ServerRef::new("Example.ORG", None).unwrap().host,
            "example.org"
        );
        for bad in [
            "",
            "-a.org",
            "a-.org",
            "a..org",
            "host:0",
            "host:70000",
            "host:",
            "ex ample.org",
            "пример.рф",
            "a/b",
            "user@host",
        ] {
            assert!(ServerRef::new(bad, None).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_missing_server_duplicates_and_bad_values() {
        let uri = invite(InviteMethod::Link, None).to_uri();
        assert!(Invite::parse(&format!("{uri}&m=q")).is_err());
        assert!(Invite::parse(&uri.replace("v=1", "v=2")).is_err());
        assert!(Invite::parse(&uri.replace("staya://", "https://")).is_err());
        assert!(Invite::parse(&uri.replace("s=staya.example.org&", "")).is_err());
        assert!(Invite::parse(&format!("{uri}&p=short")).is_err());
    }

    #[test]
    fn debug_hides_token() {
        let mut inv = invite(InviteMethod::Qr, None);
        inv.token = [0xEE; 16];
        assert!(!format!("{inv:?}").contains("238"));
    }
}
