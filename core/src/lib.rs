//! Staya core: identity keys, Olm/Megolm sessions, location packets and local storage.

pub mod account;
pub mod friends;
pub mod safety;
pub mod store;

uniffi::setup_scaffolding!();

/// Ошибки ядра. Сообщения не содержат секретов и координат.
#[derive(Debug, thiserror::Error)]
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
    #[error("unknown friend")]
    UnknownFriend,
    #[error("invalid invite: {0}")]
    InvalidInvite(&'static str),
    #[error(transparent)]
    Proto(#[from] staya_proto::ProtoError),
}

/// Версия ядра — проверка, что конвейер Rust → Swift/Kotlin работает.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_package() {
        assert_eq!(core_version(), "0.1.0");
    }
}
