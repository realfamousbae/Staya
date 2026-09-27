//! Staya core: identity keys, Olm/Megolm sessions, location packets and local storage.

uniffi::setup_scaffolding!();

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
