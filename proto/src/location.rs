//! Открытый текст пакета позиции (§7.1) и сетка `Approx` (§7.3).

use std::fmt;

use crate::ProtoError;
use crate::bytes::Reader;
use crate::consts::LOCATION_PLAINTEXT_LEN;

const VERSION: u8 = 1;

/// Режим пакета позиции.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LocationKind {
    Exact = 0,
    Approx = 1,
    Hidden = 2,
    Frozen = 3,
}

impl TryFrom<u8> for LocationKind {
    type Error = ProtoError;

    fn try_from(v: u8) -> Result<Self, ProtoError> {
        match v {
            0 => Ok(Self::Exact),
            1 => Ok(Self::Approx),
            2 => Ok(Self::Hidden),
            3 => Ok(Self::Frozen),
            other => Err(ProtoError::UnknownType(other)),
        }
    }
}

/// Позиция в пакете. Координаты — градусы × 10⁷.
///
/// `Debug` не печатает координаты: они не должны попадать в логи.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LocationPayload {
    pub kind: LocationKind,
    pub lat_e7: i32,
    pub lon_e7: i32,
    pub accuracy_m: u16,
    pub timestamp: i64,
}

impl fmt::Debug for LocationPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocationPayload")
            .field("kind", &self.kind)
            .field("coordinates", &"<redacted>")
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

const LAT_MAX_E7: i32 = 900_000_000;
const LON_MAX_E7: i32 = 1_800_000_000;

impl LocationPayload {
    /// Пакет `Hidden`: без координат.
    pub fn hidden(timestamp: i64) -> Self {
        Self {
            kind: LocationKind::Hidden,
            lat_e7: 0,
            lon_e7: 0,
            accuracy_m: 0,
            timestamp,
        }
    }

    pub fn encode(&self) -> Result<[u8; LOCATION_PLAINTEXT_LEN], ProtoError> {
        self.validate()?;
        let mut out = [0u8; LOCATION_PLAINTEXT_LEN];
        out[0] = VERSION;
        out[1] = self.kind as u8;
        out[2..6].copy_from_slice(&self.lat_e7.to_be_bytes());
        out[6..10].copy_from_slice(&self.lon_e7.to_be_bytes());
        out[10..12].copy_from_slice(&self.accuracy_m.to_be_bytes());
        out[12..20].copy_from_slice(&self.timestamp.to_be_bytes());
        // 20..22 — flags = 0, 22..48 — резерв = 0.
        Ok(out)
    }

    pub fn decode(buf: &[u8]) -> Result<Self, ProtoError> {
        if buf.len() != LOCATION_PLAINTEXT_LEN {
            return Err(ProtoError::Length {
                expected: LOCATION_PLAINTEXT_LEN,
                got: buf.len(),
            });
        }
        let mut r = Reader::new(buf);
        let version = r.u8()?;
        if version != VERSION {
            return Err(ProtoError::Version(version));
        }
        let kind = LocationKind::try_from(r.u8()?)?;
        let payload = Self {
            kind,
            lat_e7: r.i32()?,
            lon_e7: r.i32()?,
            accuracy_m: r.u16()?,
            timestamp: r.i64()?,
        };
        // flags и резерв при приёме игнорируются (§7.1).
        payload.validate()?;
        Ok(payload)
    }

    fn validate(&self) -> Result<(), ProtoError> {
        if !(-LAT_MAX_E7..=LAT_MAX_E7).contains(&self.lat_e7) {
            return Err(ProtoError::Invalid("latitude"));
        }
        if !(-LON_MAX_E7..=LON_MAX_E7).contains(&self.lon_e7) {
            return Err(ProtoError::Invalid("longitude"));
        }
        if self.kind == LocationKind::Hidden && (self.lat_e7 != 0 || self.lon_e7 != 0) {
            return Err(ProtoError::Invalid("hidden payload carries coordinates"));
        }
        Ok(())
    }
}

/// Шаг сетки `Approx` по широте: 0,01°.
pub const APPROX_LAT_STEP_E7: i64 = 100_000;

/// Привязывает точку к центру ячейки сетки `Approx` (§7.3).
///
/// Возвращает координаты центра ячейки и `accuracy` — половину её диагонали в метрах.
pub fn snap_to_grid(lat_e7: i32, lon_e7: i32) -> (i32, i32, u16) {
    let lat_step = APPROX_LAT_STEP_E7;
    let lat_cell = (lat_e7 as i64).div_euclid(lat_step);
    let lat_center =
        (lat_cell * lat_step + lat_step / 2).clamp(-(LAT_MAX_E7 as i64), LAT_MAX_E7 as i64);

    let cos = ((lat_center as f64) / 1e7).to_radians().cos().max(0.1);
    // Шаг по долготе в тех же единицах; округление детерминировано для отправителя.
    let lon_step = ((lat_step as f64) / cos).round() as i64;
    let lon_cell = (lon_e7 as i64).div_euclid(lon_step);
    let lon_center =
        (lon_cell * lon_step + lon_step / 2).clamp(-(LON_MAX_E7 as i64), LON_MAX_E7 as i64);

    // Ячейка ≈ 1,11 км по широте и столько же по долготе (с учётом cos).
    const METERS_PER_DEG: f64 = 111_320.0;
    let height = lat_step as f64 / 1e7 * METERS_PER_DEG;
    let width = lon_step as f64 / 1e7 * METERS_PER_DEG * cos;
    let half_diag = (height.hypot(width) / 2.0).round().min(u16::MAX as f64) as u16;

    (lat_center as i32, lon_center as i32, half_diag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn kind() -> impl Strategy<Value = LocationKind> {
        prop_oneof![
            Just(LocationKind::Exact),
            Just(LocationKind::Approx),
            Just(LocationKind::Frozen),
        ]
    }

    proptest! {
        #[test]
        fn roundtrip(
            kind in kind(),
            lat in -LAT_MAX_E7..=LAT_MAX_E7,
            lon in -LON_MAX_E7..=LON_MAX_E7,
            acc: u16,
            ts: i64,
        ) {
            let p = LocationPayload { kind, lat_e7: lat, lon_e7: lon, accuracy_m: acc, timestamp: ts };
            let bytes = p.encode().unwrap();
            prop_assert_eq!(bytes.len(), LOCATION_PLAINTEXT_LEN);
            prop_assert_eq!(LocationPayload::decode(&bytes).unwrap(), p);
        }

        #[test]
        fn decode_never_panics(buf in proptest::collection::vec(any::<u8>(), 0..100)) {
            let _ = LocationPayload::decode(&buf);
        }

        #[test]
        fn snap_is_stable_within_cell(
            lat in -890_000_000i32..=890_000_000,
            lon in -1_790_000_000i32..=1_790_000_000,
        ) {
            let (clat, clon, acc) = snap_to_grid(lat, lon);
            // Центр ячейки привязывается сам к себе.
            prop_assert_eq!(snap_to_grid(clat, clon), (clat, clon, acc));
            // Точка не дальше половины диагонали от центра (с запасом на округление).
            let dlat = (lat - clat) as f64 / 1e7 * 111_320.0;
            let dlon = (lon - clon) as f64 / 1e7 * 111_320.0 * (clat as f64 / 1e7).to_radians().cos().max(0.1);
            prop_assert!(dlat.hypot(dlon) <= acc as f64 + 2.0);
        }
    }

    #[test]
    fn hidden_has_no_coordinates() {
        let bytes = LocationPayload::hidden(1).encode().unwrap();
        assert_eq!(&bytes[2..10], &[0; 8]);
        let bad = LocationPayload {
            lat_e7: 1,
            ..LocationPayload::hidden(1)
        };
        assert!(bad.encode().is_err());
    }

    #[test]
    fn rejects_unknown_version_and_kind() {
        let mut bytes = LocationPayload::hidden(1).encode().unwrap();
        bytes[0] = 2;
        assert_eq!(LocationPayload::decode(&bytes), Err(ProtoError::Version(2)));
        bytes[0] = 1;
        bytes[1] = 9;
        assert_eq!(
            LocationPayload::decode(&bytes),
            Err(ProtoError::UnknownType(9))
        );
    }

    #[test]
    fn debug_redacts_coordinates() {
        let p = LocationPayload {
            kind: LocationKind::Exact,
            lat_e7: 557_558_000,
            lon_e7: 376_173_000,
            accuracy_m: 5,
            timestamp: 0,
        };
        let s = format!("{p:?}");
        assert!(!s.contains("557558000") && !s.contains("376173000"));
        assert!(s.contains("<redacted>"));
    }

    #[test]
    fn approx_cell_is_about_a_kilometre() {
        // Москва, ~55,75° с. ш.
        let (_, _, acc) = snap_to_grid(557_558_000, 376_173_000);
        assert!((700..=900).contains(&acc), "half diagonal {acc} m");
    }
}
