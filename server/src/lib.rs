//! Сервер Staya: хранит только зашифрованные почтовые ящики и публичные ключи.

pub mod auth;
pub mod db;
pub mod envelopes;
pub mod http;
pub mod keys;
pub mod mailbox;

#[cfg(feature = "dev")]
pub mod dev;
