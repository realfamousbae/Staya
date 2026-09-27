//! Реальные сообщения vodozemac 0.11 помещаются в конверты протокола (§6.2, §7.2).

use staya_proto::consts::{CONTROL_BUCKETS, LOCATION_ENVELOPE_LEN, LOCATION_PLAINTEXT_LEN};
use staya_proto::envelope::{ControlEnvelope, LocationEnvelope, OlmType};
use vodozemac::megolm::{GroupSession, SessionConfig as MegolmConfig};
use vodozemac::olm::{Account, OlmMessage, SessionConfig as OlmConfig};

/// Индекс Megolm кодируется varint; при индексе 0 он короче всего на 4 байта,
/// чем при максимальном u32. Проверяем запас с учётом худшего случая.
const VARINT_WORST_EXTRA: usize = 4;

#[test]
fn megolm_location_fits() {
    let mut s = GroupSession::new(MegolmConfig::version_1());
    let msg = s.encrypt([0u8; LOCATION_PLAINTEXT_LEN]).to_bytes();
    let worst = msg.len() + VARINT_WORST_EXTRA;
    assert!(
        worst + 2 <= LOCATION_ENVELOPE_LEN,
        "worst case {worst} bytes"
    );
    let env = LocationEnvelope::seal(&msg).unwrap();
    assert_eq!(env.open().unwrap(), &msg[..]);
}

#[test]
fn olm_control_buckets_fit() {
    let alice = Account::new();
    let mut bob = Account::new();
    bob.generate_one_time_keys(1);
    let otk = *bob.one_time_keys().values().next().unwrap();
    let mut session = alice
        .create_outbound_session(OlmConfig::version_1(), bob.curve25519_key(), otk)
        .unwrap();

    for bucket in CONTROL_BUCKETS {
        // PreKey — самый длинный вид Olm-сообщения.
        let OlmMessage::PreKey(m) = session.encrypt(vec![0u8; bucket.plaintext]).unwrap() else {
            panic!("expected a pre-key message");
        };
        let bytes = m.to_bytes();
        // Счётчик цепочки тоже varint: закладываем тот же худший случай.
        let worst = bytes.len() + VARINT_WORST_EXTRA;
        assert!(
            worst + 3 <= bucket.envelope,
            "bucket {}: worst case {worst} bytes, envelope {}",
            bucket.plaintext,
            bucket.envelope
        );
        let env = ControlEnvelope::seal(OlmType::PreKey, &bytes).unwrap();
        assert_eq!(env.as_bytes().len(), bucket.envelope);
    }
}
