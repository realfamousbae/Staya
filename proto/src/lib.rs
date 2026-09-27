//! Wire formats shared by the Staya core and server. Contains no cryptography.
//!
//! Every layout here implements `docs/protocol.md` v0.1; the section numbers in
//! comments refer to that document.

pub mod api;
pub mod consts;
pub mod control;
pub mod envelope;
pub mod ids;
pub mod invite;
pub mod location;
pub mod signing;

mod bytes;

pub use ids::AccountId;

/// Ошибка разбора или сборки структуры протокола.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtoError {
    #[error("unexpected length: expected {expected}, got {got}")]
    Length { expected: usize, got: usize },
    #[error("unsupported version {0}")]
    Version(u8),
    #[error("unknown type or kind {0}")]
    UnknownType(u8),
    #[error("truncated input")]
    Truncated,
    #[error("field too large: {0}")]
    TooLarge(&'static str),
    #[error("invalid field: {0}")]
    Invalid(&'static str),
}
