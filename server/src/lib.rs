//! Сервер Staya: хранит только зашифрованные почтовые ящики и публичные ключи.

pub mod mailbox;

#[cfg(feature = "dev")]
pub mod dev;
