//! Код безопасности (`docs/protocol.md` §5.2) по схеме численного отпечатка Signal.

use sha2::{Digest, Sha512};
use staya_proto::AccountId;
use staya_proto::consts::FINGERPRINT_ITERATIONS;

use crate::account::Identity;

const CONTEXT: &[u8] = b"staya/v1/fingerprint\0";

/// 30 цифр отпечатка одной стороны.
fn party_digits(id: &Identity) -> String {
    let mut h = Sha512::new()
        .chain_update(CONTEXT)
        .chain_update(id.ik)
        .chain_update(id.sk)
        .chain_update(id.account_id.0)
        .finalize();
    for _ in 1..FINGERPRINT_ITERATIONS {
        h = Sha512::new()
            .chain_update(h)
            .chain_update(id.ik)
            .chain_update(id.sk)
            .finalize();
    }
    h[..30]
        .chunks(5)
        .map(|c| {
            let n = c.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64);
            format!("{:05}", n % 100_000)
        })
        .collect()
}

/// Код безопасности пары: 60 цифр, одинаковый на обоих устройствах.
pub fn safety_code(a: &Identity, b: &Identity) -> String {
    let (first, second) = order(a, b);
    let mut code = party_digits(first);
    code.push_str(&party_digits(second));
    code
}

fn order<'a>(a: &'a Identity, b: &'a Identity) -> (&'a Identity, &'a Identity) {
    if cmp_ids(&a.account_id, &b.account_id).is_le() {
        (a, b)
    } else {
        (b, a)
    }
}

fn cmp_ids(a: &AccountId, b: &AccountId) -> std::cmp::Ordering {
    a.0.cmp(&b.0)
}

/// Разбивает код на группы по 5 цифр для показа.
pub fn format_groups(code: &str) -> String {
    code.as_bytes()
        .chunks(5)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(b: u8) -> Identity {
        Identity {
            account_id: AccountId([b; 16]),
            ik: [b; 32],
            sk: [b.wrapping_add(1); 32],
        }
    }

    #[test]
    fn symmetric_and_sixty_digits() {
        let (a, b) = (identity(1), identity(2));
        let code = safety_code(&a, &b);
        assert_eq!(code, safety_code(&b, &a));
        assert_eq!(code.len(), 60);
        assert!(code.bytes().all(|c| c.is_ascii_digit()));
        assert_eq!(format_groups(&code).split(' ').count(), 12);
    }

    #[test]
    fn changes_when_any_key_changes() {
        let (a, b) = (identity(1), identity(2));
        let base = safety_code(&a, &b);
        let mut other = b;
        other.sk[0] ^= 1;
        assert_ne!(base, safety_code(&a, &other));
        let mut other = b;
        other.ik[31] ^= 1;
        assert_ne!(base, safety_code(&a, &other));
    }
}
