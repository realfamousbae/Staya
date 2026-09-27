//! Аккаунт устройства: ключи идентичности, подписи, одноразовые и fallback-ключи
//! (`docs/protocol.md` §3, §4).

use serde::{Deserialize, Serialize};
use staya_proto::AccountId;
use staya_proto::api::{B64, PublishKeysRequest, RegisterRequest, SignedKey};
use staya_proto::consts::{FALLBACK_ROTATION, OTK_REFILL_BELOW, OTK_TARGET};
use staya_proto::signing;
use vodozemac::olm::{Account, AccountPickle};
use zeroize::Zeroizing;

use crate::CoreError;
use crate::store::Store;

const RECORD: &str = "account";

/// Публичные ключи аккаунта.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub account_id: AccountId,
    /// Curve25519.
    pub ik: [u8; 32],
    /// Ed25519.
    pub sk: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct Persisted {
    account_id: [u8; 16],
    /// Unix-время последней ротации fallback-ключа.
    fallback_rotated_at: Option<i64>,
    olm: AccountPickle,
}

pub struct LocalAccount {
    account_id: AccountId,
    fallback_rotated_at: Option<i64>,
    olm: Account,
}

impl LocalAccount {
    /// Создаёт новый аккаунт и сохраняет его. Ошибка, если аккаунт уже есть.
    pub fn create(store: &Store) -> Result<Self, CoreError> {
        if store.get_secret(RECORD)?.is_some() {
            return Err(CoreError::AccountExists);
        }
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|_| CoreError::Random)?;
        let account = Self {
            account_id: AccountId(id),
            fallback_rotated_at: None,
            olm: Account::new(),
        };
        account.save(store)?;
        Ok(account)
    }

    pub fn load(store: &Store) -> Result<Option<Self>, CoreError> {
        let Some(bytes) = store.get_secret(RECORD)? else {
            return Ok(None);
        };
        let p: Persisted = serde_json::from_slice(&bytes).map_err(|_| CoreError::Corrupted)?;
        Ok(Some(Self {
            account_id: AccountId(p.account_id),
            fallback_rotated_at: p.fallback_rotated_at,
            olm: Account::from_pickle(p.olm),
        }))
    }

    fn save(&self, store: &Store) -> Result<(), CoreError> {
        let p = Persisted {
            account_id: self.account_id.0,
            fallback_rotated_at: self.fallback_rotated_at,
            olm: self.olm.pickle(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&p).map_err(|_| CoreError::Corrupted)?);
        store.put_secret(RECORD, &bytes)
    }

    pub fn identity(&self) -> Identity {
        Identity {
            account_id: self.account_id,
            ik: self.olm.curve25519_key().to_bytes(),
            sk: self.olm.ed25519_key().as_bytes().to_owned(),
        }
    }

    fn sign(&self, message: &[u8]) -> B64 {
        B64(self.olm.sign(message).to_bytes().to_vec())
    }

    /// Запрос регистрации на сервере (§4.1).
    pub fn register_request(&self, invite_code: Option<String>) -> RegisterRequest {
        let id = self.identity();
        RegisterRequest {
            account_id: id.account_id,
            ik: B64(id.ik.to_vec()),
            sk: B64(id.sk.to_vec()),
            signature: self.sign(&signing::register(&id.account_id, &id.ik, &id.sk)),
            invite_code,
        }
    }

    /// Подпись challenge для входа (§4.2).
    pub fn sign_auth(&self, domain: &str, nonce: &[u8; 32]) -> Result<B64, CoreError> {
        Ok(self.sign(&signing::auth(domain, nonce, &self.account_id)?))
    }

    /// Ключи для публикации на сервере или `None`, если публиковать нечего.
    ///
    /// `server_otk_count` — сколько наших OTK сейчас на сервере. Сгенерированные
    /// ключи сохраняются до возврата, поэтому сбой сети не приводит к потере
    /// приватных ключей для уже опубликованных OTK. Пока сервер не подтвердил
    /// публикацию ([`Self::mark_keys_published`]), повторный вызов вернёт те же
    /// ключи, а не создаст новые.
    pub fn keys_to_publish(
        &mut self,
        store: &Store,
        server_otk_count: usize,
        now: i64,
    ) -> Result<Option<PublishKeysRequest>, CoreError> {
        let mut changed = false;

        let pending = self.olm.one_time_keys().len();
        if pending == 0 && server_otk_count < OTK_REFILL_BELOW {
            self.olm
                .generate_one_time_keys(OTK_TARGET.saturating_sub(server_otk_count));
            changed = true;
        }

        let rotation_due = match self.fallback_rotated_at {
            None => true,
            Some(at) => now.saturating_sub(at) >= FALLBACK_ROTATION.as_secs() as i64,
        };
        if rotation_due && self.olm.fallback_key().is_empty() {
            // vodozemac хранит и предыдущий fallback-ключ, пока не появится следующий.
            self.olm.generate_fallback_key();
            self.fallback_rotated_at = Some(now);
            changed = true;
        }

        if changed {
            self.save(store)?;
        }

        let one_time_keys: Vec<SignedKey> = self
            .olm
            .one_time_keys()
            .values()
            .map(|k| {
                let key = k.to_bytes();
                SignedKey {
                    key: B64(key.to_vec()),
                    signature: self.sign(&signing::one_time_key(&key)),
                }
            })
            .collect();
        let fallback_key = self.olm.fallback_key().values().next().map(|k| {
            let key = k.to_bytes();
            SignedKey {
                key: B64(key.to_vec()),
                signature: self.sign(&signing::fallback_key(&key)),
            }
        });

        if one_time_keys.is_empty() && fallback_key.is_none() {
            return Ok(None);
        }
        Ok(Some(PublishKeysRequest {
            one_time_keys,
            fallback_key,
        }))
    }

    /// Сервер принял ключи из [`Self::keys_to_publish`].
    pub fn mark_keys_published(&mut self, store: &Store) -> Result<(), CoreError> {
        self.olm.mark_keys_as_published();
        self.save(store)
    }

    #[cfg(test)]
    pub(crate) fn olm(&self) -> &Account {
        &self.olm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::DbKey;
    use vodozemac::{Ed25519PublicKey, Ed25519Signature};

    fn store() -> Store {
        Store::open_in_memory(&DbKey::new([9; 32])).unwrap()
    }

    fn verify(sk: &[u8; 32], msg: &[u8], sig: &B64) -> bool {
        let pk = Ed25519PublicKey::from_slice(sk).unwrap();
        let sig = Ed25519Signature::from_slice(&sig.0).unwrap();
        pk.verify(msg, &sig).is_ok()
    }

    #[test]
    fn create_load_and_refuse_duplicate() {
        let s = store();
        let a = LocalAccount::create(&s).unwrap();
        let loaded = LocalAccount::load(&s).unwrap().unwrap();
        assert_eq!(a.identity(), loaded.identity());
        assert!(matches!(
            LocalAccount::create(&s),
            Err(CoreError::AccountExists)
        ));
    }

    #[test]
    fn register_and_auth_signatures_verify() {
        let s = store();
        let a = LocalAccount::create(&s).unwrap();
        let id = a.identity();

        let req = a.register_request(Some("beta".into()));
        assert!(verify(
            &id.sk,
            &signing::register(&id.account_id, &id.ik, &id.sk),
            &req.signature
        ));

        let nonce = [5u8; 32];
        let sig = a.sign_auth("staya.example", &nonce).unwrap();
        assert!(verify(
            &id.sk,
            &signing::auth("staya.example", &nonce, &id.account_id).unwrap(),
            &sig
        ));
        // Подпись входа не подходит к другому домену.
        assert!(!verify(
            &id.sk,
            &signing::auth("evil.example", &nonce, &id.account_id).unwrap(),
            &sig
        ));
    }

    #[test]
    fn otk_and_fallback_lifecycle() {
        let s = store();
        let mut a = LocalAccount::create(&s).unwrap();
        let sk = a.identity().sk;

        let req = a.keys_to_publish(&s, 0, 1_000).unwrap().unwrap();
        assert_eq!(req.one_time_keys.len(), OTK_TARGET);
        for k in &req.one_time_keys {
            let key: [u8; 32] = k.key.0.clone().try_into().unwrap();
            assert!(verify(&sk, &signing::one_time_key(&key), &k.signature));
        }
        let fb = req.fallback_key.clone().unwrap();
        let fb_key: [u8; 32] = fb.key.0.clone().try_into().unwrap();
        assert!(verify(&sk, &signing::fallback_key(&fb_key), &fb.signature));
        // Подпись OTK не подходит как подпись fallback-ключа.
        assert!(!verify(
            &sk,
            &signing::fallback_key(&fb_key),
            &req.one_time_keys[0].signature
        ));

        // Без подтверждения сервера — те же ключи, а не новые (и они пережили перезапуск).
        let mut reloaded = LocalAccount::load(&s).unwrap().unwrap();
        let again = reloaded.keys_to_publish(&s, 0, 1_001).unwrap().unwrap();
        assert_eq!(again.one_time_keys.len(), OTK_TARGET);
        let keys = |r: &PublishKeysRequest| {
            let mut v: Vec<_> = r.one_time_keys.iter().map(|k| k.key.0.clone()).collect();
            v.sort();
            v
        };
        assert_eq!(keys(&req), keys(&again));

        reloaded.mark_keys_published(&s).unwrap();
        // На сервере достаточно ключей, fallback свежий — публиковать нечего.
        assert!(reloaded.keys_to_publish(&s, 45, 2_000).unwrap().is_none());
        // Осталось мало — дополняем до 50.
        let refill = reloaded.keys_to_publish(&s, 10, 2_000).unwrap().unwrap();
        assert_eq!(refill.one_time_keys.len(), OTK_TARGET - 10);
        assert!(refill.fallback_key.is_none());
        reloaded.mark_keys_published(&s).unwrap();

        // Через 7 дней — новый fallback-ключ.
        let week = FALLBACK_ROTATION.as_secs() as i64;
        let rotated = reloaded
            .keys_to_publish(&s, 45, 1_000 + week)
            .unwrap()
            .unwrap();
        assert!(rotated.one_time_keys.is_empty());
        assert_ne!(rotated.fallback_key.unwrap().key, fb.key);
    }

    #[test]
    fn identity_matches_olm_keys() {
        let s = store();
        let a = LocalAccount::create(&s).unwrap();
        assert_eq!(a.identity().ik, a.olm().curve25519_key().to_bytes());
    }
}
