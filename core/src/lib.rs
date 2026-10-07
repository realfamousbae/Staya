//! Staya core: identity keys, Olm/Megolm sessions, location packets and local storage.

pub mod account;
pub mod api;
pub mod friends;
#[cfg(any(test, feature = "fuzzing"))]
pub mod fuzzing;
pub mod safety;
pub mod server;
pub mod store;

uniffi::setup_scaffolding!();

/// Ошибки ядра. Сообщения не содержат секретов и координат; в Swift/Kotlin
/// приходят как исключение с текстом (`flat_error`).
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum CoreError {
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("unsupported local database schema version {0}")]
    SchemaVersion(i64),
    #[error("local data is corrupted or the database key is wrong")]
    Corrupted,
    #[error("invalid database key")]
    InvalidKey,
    #[error("system random generator failed")]
    Random,
    #[error("cryptographic operation failed: {0}")]
    Crypto(&'static str),
    #[error("an account already exists on this device")]
    AccountExists,
    #[error("a location is required unless ghost mode or freeze is on")]
    MissingLocation,
    #[error("invalid input: {0}")]
    Invalid(&'static str),
    #[error("the core panicked earlier; open it again")]
    Poisoned,
    #[error("unknown friend")]
    UnknownFriend,
    #[error("invalid invite: {0}")]
    InvalidInvite(&'static str),
    #[error(transparent)]
    Proto(#[from] staya_proto::ProtoError),
    #[error("this account is bound to another server")]
    ServerMismatch,
    #[error("no server is set for this account")]
    NoServer,
}

/// Версия ядра — проверка, что конвейер Rust → Swift/Kotlin работает.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Ссылка Staya в любом виде (`staya://…` или https со ссылкой во фрагменте,
/// protocol §5.4) → `staya://add?…` / `staya://server?…`; не ссылка Staya — `None`.
/// Для разбора вставленного текста и открытых ссылок.
#[uniffi::export]
pub fn normalize_link(text: String) -> Option<String> {
    staya_proto::invite::normalize_link(&text)
}

/// https-вид ссылки для отправки в мессенджер (§5.4); не ссылка Staya — `None`.
#[uniffi::export]
pub fn web_link(uri: String) -> Option<String> {
    staya_proto::invite::to_web_link(&uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_package() {
        assert_eq!(core_version(), "0.1.0");
    }
}
