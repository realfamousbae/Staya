//! Идентификаторы (§3).

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ProtoError;

/// 128-битный случайный ID аккаунта. В тексте — base64url без выравнивания.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountId(pub [u8; 16]);

impl AccountId {
    pub const LEN: usize = 16;

    pub fn to_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_b64(s: &str) -> Result<Self, ProtoError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|_| ProtoError::Invalid("account id"))?;
        let arr: [u8; 16] = bytes
            .try_into()
            .map_err(|_| ProtoError::Invalid("account id"))?;
        Ok(Self(arr))
    }
}

impl fmt::Debug for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountId({})", self.to_b64())
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_b64())
    }
}

impl Serialize for AccountId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_b64())
    }
}

impl<'de> Deserialize<'de> for AccountId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::from_b64(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64_roundtrip() {
        let id = AccountId([7; 16]);
        assert_eq!(id.to_b64().len(), 22);
        assert_eq!(AccountId::from_b64(&id.to_b64()), Ok(id));
        assert!(AccountId::from_b64("short").is_err());
    }
}
