//! Привязка аккаунта к серверу (protocol §5.3): имя, отпечатки ключей TLS,
//! ключ, запомненный при первом подключении (TOFU), и токен сессии. Всё — в
//! зашифрованной базе: токен — секрет, а запомненный ключ нельзя подменить
//! записью в незащищённый файл.

use serde::{Deserialize, Serialize};
use staya_proto::invite::ServerRef;
use zeroize::Zeroizing;

use crate::CoreError;
use crate::store::Store;

const RECORD: &str = "server";

#[derive(Serialize, Deserialize)]
struct Persisted {
    host: String,
    pins: Vec<[u8; 32]>,
    learned: Option<[u8; 32]>,
    session: Option<Session>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Session {
    token: Vec<u8>,
    expires_at: i64,
}

/// Решение о ключе сервера (правило доверия §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ServerTrust {
    /// Совпал с отпечатком или с запомненным ключом.
    Trusted,
    /// Отпечатков нет, ключ увиден впервые и запомнен.
    Learned,
    /// Ключ не совпал: отказать в подключении. Для TOFU — «ключ сервера изменился».
    Rejected,
}

pub struct ServerBinding {
    server: ServerRef,
    learned: Option<[u8; 32]>,
    session: Option<Session>,
}

impl ServerBinding {
    pub fn load(store: &Store) -> Result<Option<Self>, CoreError> {
        let Some(bytes) = store.get_secret(RECORD)? else {
            return Ok(None);
        };
        let p: Persisted = serde_json::from_slice(&bytes).map_err(|_| CoreError::Corrupted)?;
        Ok(Some(Self {
            server: ServerRef::new(&p.host, p.pins).map_err(|_| CoreError::Corrupted)?,
            learned: p.learned,
            session: p.session,
        }))
    }

    fn save(&self, store: &Store) -> Result<(), CoreError> {
        let p = Persisted {
            host: self.server.host.clone(),
            pins: self.server.pins.clone(),
            learned: self.learned,
            session: self.session.clone(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&p).map_err(|_| CoreError::Corrupted)?);
        store.put_secret(RECORD, &bytes)
    }

    pub fn server(&self) -> &ServerRef {
        &self.server
    }

    pub fn learned(&self) -> Option<[u8; 32]> {
        self.learned
    }

    /// Привязывает аккаунт к серверу. Аккаунт живёт на одном сервере: другой
    /// сервер — ошибка. Тот же сервер с отпечатками, когда своих ещё нет
    /// (был TOFU), — отпечатки принимаются, запомненный ключ забывается.
    pub fn bind(
        store: &Store,
        current: Option<Self>,
        server: &ServerRef,
    ) -> Result<Self, CoreError> {
        let mut binding = match current {
            Some(b) if b.server.host != server.host => return Err(CoreError::ServerMismatch),
            Some(b) => b,
            None => Self {
                server: server.clone(),
                learned: None,
                session: None,
            },
        };
        if binding.server.pins.is_empty() && !server.pins.is_empty() {
            binding.server.pins = server.pins.clone();
            binding.learned = None;
        }
        binding.save(store)?;
        Ok(binding)
    }

    /// Правило доверия §5.3 для SHA-256 SPKI предъявленного ключа. Вызывается
    /// после обычной проверки цепочки сертификата и до отправки запроса.
    pub fn check_key(
        &mut self,
        store: &Store,
        spki_sha256: &[u8; 32],
    ) -> Result<ServerTrust, CoreError> {
        if !self.server.pins.is_empty() {
            return Ok(if self.server.pins.contains(spki_sha256) {
                ServerTrust::Trusted
            } else {
                ServerTrust::Rejected
            });
        }
        match self.learned {
            Some(k) if &k == spki_sha256 => Ok(ServerTrust::Trusted),
            // Запомненный ключ молча не заменяется (§5.3).
            Some(_) => Ok(ServerTrust::Rejected),
            None => {
                self.learned = Some(*spki_sha256);
                self.save(store)?;
                Ok(ServerTrust::Learned)
            }
        }
    }

    /// Действующий токен сессии.
    pub fn session(&self, now: i64) -> Option<&[u8]> {
        self.session
            .as_ref()
            .filter(|s| s.expires_at > now)
            .map(|s| s.token.as_slice())
    }

    pub fn set_session(
        &mut self,
        store: &Store,
        token: Vec<u8>,
        expires_at: i64,
    ) -> Result<(), CoreError> {
        self.session = Some(Session { token, expires_at });
        self.save(store)
    }

    pub fn clear_session(&mut self, store: &Store) -> Result<(), CoreError> {
        self.session = None;
        self.save(store)
    }

    /// Имя сервера для подписи входа (§4.2): хост без порта.
    pub fn auth_domain(&self) -> &str {
        self.server
            .host
            .rsplit_once(':')
            .map_or(self.server.host.as_str(), |(h, _)| h)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.token);
    }
}
