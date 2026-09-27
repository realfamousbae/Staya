//! Конверты фиксированного размера (§6.2, §7.2).
//!
//! Конверт — это `len`[u16] ‖ байты сообщения ‖ нули. Фиксированный размер
//! скрывает от сервера длину сообщения (в том числе длину varint-индекса).

use crate::ProtoError;
use crate::bytes::Reader;
use crate::consts::{CONTROL_BUCKETS, LOCATION_ENVELOPE_LEN, is_control_envelope_len};

/// Тип Olm-сообщения в управляющем конверте.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OlmType {
    PreKey = 0,
    Normal = 1,
}

/// Конверт пакета позиции: ровно 160 байт.
#[derive(Clone, PartialEq, Eq)]
pub struct LocationEnvelope(pub [u8; LOCATION_ENVELOPE_LEN]);

impl std::fmt::Debug for LocationEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocationEnvelope(160 bytes)")
    }
}

impl LocationEnvelope {
    /// Упаковывает байты `MegolmMessage`.
    pub fn seal(megolm_message: &[u8]) -> Result<Self, ProtoError> {
        let mut out = [0u8; LOCATION_ENVELOPE_LEN];
        let len = megolm_message.len();
        if len + 2 > LOCATION_ENVELOPE_LEN {
            return Err(ProtoError::TooLarge("megolm message"));
        }
        out[..2].copy_from_slice(&(len as u16).to_be_bytes());
        out[2..2 + len].copy_from_slice(megolm_message);
        Ok(Self(out))
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, ProtoError> {
        let arr: [u8; LOCATION_ENVELOPE_LEN] = buf.try_into().map_err(|_| ProtoError::Length {
            expected: LOCATION_ENVELOPE_LEN,
            got: buf.len(),
        })?;
        Ok(Self(arr))
    }

    /// Байты `MegolmMessage` внутри конверта.
    pub fn open(&self) -> Result<&[u8], ProtoError> {
        let mut r = Reader::new(&self.0);
        let len = r.u16()? as usize;
        r.take(len)
    }
}

/// Конверт управляющего сообщения: 512, 1280 или 9472 байта.
#[derive(Clone, PartialEq, Eq)]
pub struct ControlEnvelope(Vec<u8>);

impl std::fmt::Debug for ControlEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ControlEnvelope({} bytes)", self.0.len())
    }
}

impl ControlEnvelope {
    /// Упаковывает Olm-сообщение в наименьший подходящий конверт.
    pub fn seal(olm_type: OlmType, olm_message: &[u8]) -> Result<Self, ProtoError> {
        let needed = 3 + olm_message.len();
        let size = CONTROL_BUCKETS
            .iter()
            .map(|b| b.envelope)
            .find(|&e| needed <= e)
            .ok_or(ProtoError::TooLarge("olm message"))?;
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(&(olm_message.len() as u16).to_be_bytes());
        out.push(olm_type as u8);
        out.extend_from_slice(olm_message);
        out.resize(size, 0);
        Ok(Self(out))
    }

    pub fn from_bytes(buf: &[u8]) -> Result<Self, ProtoError> {
        if !is_control_envelope_len(buf.len()) {
            return Err(ProtoError::Invalid("control envelope size"));
        }
        Ok(Self(buf.to_vec()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Тип и байты Olm-сообщения внутри конверта.
    pub fn open(&self) -> Result<(OlmType, &[u8]), ProtoError> {
        let mut r = Reader::new(&self.0);
        let len = r.u16()? as usize;
        let olm_type = match r.u8()? {
            0 => OlmType::PreKey,
            1 => OlmType::Normal,
            other => return Err(ProtoError::UnknownType(other)),
        };
        Ok((olm_type, r.take(len)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn location_roundtrip(msg in proptest::collection::vec(any::<u8>(), 0..=158)) {
            let env = LocationEnvelope::seal(&msg).unwrap();
            prop_assert_eq!(env.open().unwrap(), &msg[..]);
            prop_assert_eq!(LocationEnvelope::from_bytes(&env.0).unwrap(), env);
        }

        #[test]
        fn control_roundtrip(msg in proptest::collection::vec(any::<u8>(), 0..=9469), prekey: bool) {
            let t = if prekey { OlmType::PreKey } else { OlmType::Normal };
            let env = ControlEnvelope::seal(t, &msg).unwrap();
            prop_assert!(is_control_envelope_len(env.as_bytes().len()));
            prop_assert_eq!(env.open().unwrap(), (t, &msg[..]));
        }

        #[test]
        fn open_never_panics(buf in proptest::collection::vec(any::<u8>(), 160..=160)) {
            let _ = LocationEnvelope::from_bytes(&buf).unwrap().open();
        }
    }

    #[test]
    fn rejects_wrong_sizes() {
        assert!(LocationEnvelope::seal(&[0; 159]).is_err());
        assert!(LocationEnvelope::from_bytes(&[0; 161]).is_err());
        assert!(ControlEnvelope::from_bytes(&[0; 513]).is_err());
        assert!(ControlEnvelope::seal(OlmType::Normal, &[0; 9470]).is_err());
    }
}
