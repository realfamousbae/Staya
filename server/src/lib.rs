//! Сервер Staya: хранит только зашифрованные почтовые ящики и публичные ключи.

pub mod auth;
pub mod db;
pub mod http;
pub mod mailbox;

#[cfg(feature = "dev")]
pub mod dev;
