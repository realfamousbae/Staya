//! Локальное хранилище ядра.
//!
//! SQLite-файл, в котором каждая секретная запись отдельно зашифрована
//! XChaCha20-Poly1305. Ключ базы (32 байта) выдаёт платформа из Keychain /
//! Android Keystore. Имя записи входит в AAD, поэтому зашифрованные значения
//! нельзя переставить между записями.

use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rusqlite::{Connection, OptionalExtension, params};
use zeroize::Zeroizing;

use crate::CoreError;

const NONCE_LEN: usize = 24;
const AAD_CONTEXT: &[u8] = b"staya/v1/store\0";
const SCHEMA_VERSION: i64 = 1;

/// Ключ локальной базы.
pub struct DbKey(Zeroizing<[u8; 32]>);

impl DbKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, CoreError> {
        let arr: [u8; 32] = bytes.try_into().map_err(|_| CoreError::InvalidKey)?;
        Ok(Self::new(arr))
    }
}

pub struct Store {
    conn: Connection,
    cipher: XChaCha20Poly1305,
}

impl Store {
    pub fn open(path: &Path, key: &DbKey) -> Result<Self, CoreError> {
        Self::init(Connection::open(path)?, key)
    }

    #[cfg(test)]
    pub fn open_in_memory(key: &DbKey) -> Result<Self, CoreError> {
        Self::init(Connection::open_in_memory()?, key)
    }

    fn init(conn: Connection, key: &DbKey) -> Result<Self, CoreError> {
        // Удалённые страницы затираются нулями, журнал не остаётся на диске.
        conn.execute_batch("PRAGMA secure_delete = ON; PRAGMA journal_mode = DELETE;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        match version {
            0 => {
                conn.execute_batch(
                    "CREATE TABLE secrets (
                        name  TEXT PRIMARY KEY,
                        value BLOB NOT NULL
                    );",
                )?;
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            SCHEMA_VERSION => {}
            other => return Err(CoreError::SchemaVersion(other)),
        }
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.0.as_ref()).map_err(|_| CoreError::InvalidKey)?;
        Ok(Self { conn, cipher })
    }

    /// Шифрует и сохраняет запись.
    pub fn put_secret(&self, name: &str, value: &[u8]) -> Result<(), CoreError> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| CoreError::Random)?;
        let aad = aad(name);
        let ciphertext = self
            .cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: value,
                    aad: &aad,
                },
            )
            .map_err(|_| CoreError::Crypto("store encrypt"))?;
        let blob = [&nonce[..], &ciphertext].concat();
        self.conn.execute(
            "INSERT INTO secrets (name, value) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value",
            params![name, blob],
        )?;
        Ok(())
    }

    /// Читает и расшифровывает запись. Неверный ключ или подмена — ошибка.
    pub fn get_secret(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, CoreError> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT value FROM secrets WHERE name = ?1",
                params![name],
                |r| r.get(0),
            )
            .optional()?;
        let Some(blob) = blob else { return Ok(None) };
        if blob.len() < NONCE_LEN {
            return Err(CoreError::Corrupted);
        }
        let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
        let aad = aad(name);
        let plaintext = self
            .cipher
            .decrypt(
                &XNonce::try_from(nonce).map_err(|_| CoreError::Corrupted)?,
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| CoreError::Corrupted)?;
        Ok(Some(Zeroizing::new(plaintext)))
    }

    /// Выполняет несколько записей атомарно: либо все, либо ни одной.
    /// Вложенные вызовы не поддерживаются.
    pub fn atomically<T>(&self, f: impl FnOnce() -> Result<T, CoreError>) -> Result<T, CoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let value = f()?;
        tx.commit()?;
        Ok(value)
    }

    pub fn delete_secret(&self, name: &str) -> Result<(), CoreError> {
        self.conn
            .execute("DELETE FROM secrets WHERE name = ?1", params![name])?;
        Ok(())
    }
}

fn aad(name: &str) -> Vec<u8> {
    [AAD_CONTEXT, name.as_bytes()].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> DbKey {
        DbKey::new([b; 32])
    }

    #[test]
    fn roundtrip_and_overwrite() {
        let s = Store::open_in_memory(&key(1)).unwrap();
        assert!(s.get_secret("a").unwrap().is_none());
        s.put_secret("a", b"one").unwrap();
        s.put_secret("a", b"two").unwrap();
        assert_eq!(s.get_secret("a").unwrap().unwrap().as_slice(), b"two");
        s.delete_secret("a").unwrap();
        assert!(s.get_secret("a").unwrap().is_none());
    }

    #[test]
    fn values_are_not_stored_in_clear() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("staya.db");
        let s = Store::open(&path, &key(1)).unwrap();
        s.put_secret("account", b"very-secret-marker").unwrap();
        drop(s);
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(18).any(|w| w == b"very-secret-marker"));
    }

    #[test]
    fn wrong_key_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("staya.db");
        Store::open(&path, &key(1))
            .unwrap()
            .put_secret("a", b"x")
            .unwrap();
        let other = Store::open(&path, &key(2)).unwrap();
        assert!(matches!(other.get_secret("a"), Err(CoreError::Corrupted)));
    }

    #[test]
    fn atomically_rolls_back_on_error() {
        let s = Store::open_in_memory(&key(1)).unwrap();
        s.put_secret("a", b"old").unwrap();
        let r: Result<(), CoreError> = s.atomically(|| {
            s.put_secret("a", b"new")?;
            s.put_secret("b", b"new")?;
            Err(CoreError::Corrupted)
        });
        assert!(r.is_err());
        assert_eq!(s.get_secret("a").unwrap().unwrap().as_slice(), b"old");
        assert!(s.get_secret("b").unwrap().is_none());
        s.atomically(|| s.put_secret("b", b"committed")).unwrap();
        assert_eq!(s.get_secret("b").unwrap().unwrap().as_slice(), b"committed");
    }

    #[test]
    fn values_cannot_be_swapped_between_names() {
        let s = Store::open_in_memory(&key(1)).unwrap();
        s.put_secret("a", b"alpha").unwrap();
        let blob: Vec<u8> = s
            .conn
            .query_row("SELECT value FROM secrets WHERE name = 'a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO secrets (name, value) VALUES ('b', ?1)",
                params![blob],
            )
            .unwrap();
        assert!(matches!(s.get_secret("b"), Err(CoreError::Corrupted)));
    }
}
