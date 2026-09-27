//! Фальшивый сервер в памяти и тестовые устройства.

#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};

use staya_core::account::LocalAccount;
use staya_core::friends::{Friends, Handled, Outgoing};
use staya_core::store::{DbKey, Store};
use staya_proto::AccountId;
use staya_proto::api::{ClaimResponse, EnvelopeKind, SignedKey};

/// Минимальная модель сервера: ключи и очередь управляющих сообщений (§8).
#[derive(Default)]
pub struct FakeServer {
    otks: BTreeMap<AccountId, Vec<SignedKey>>,
    fallback: BTreeMap<AccountId, SignedKey>,
    /// Отправитель ставится сервером — как в настоящем (§8.1).
    control: BTreeMap<AccountId, VecDeque<(AccountId, Vec<u8>)>>,
}

impl FakeServer {
    pub fn publish(&mut self, device: &mut Device, now: i64) {
        let count = self.otks.get(&device.id()).map_or(0, Vec::len);
        if let Some(req) = device
            .account
            .keys_to_publish(&device.store, count, now)
            .unwrap()
        {
            self.otks
                .entry(device.id())
                .or_default()
                .extend(req.one_time_keys);
            if let Some(fb) = req.fallback_key {
                self.fallback.insert(device.id(), fb);
            }
            device.account.mark_keys_published(&device.store).unwrap();
        }
    }

    pub fn claim(&mut self, id: AccountId) -> ClaimResponse {
        match self.otks.get_mut(&id).and_then(Vec::pop) {
            Some(key) => ClaimResponse {
                key,
                is_fallback: false,
            },
            None => ClaimResponse {
                key: self.fallback[&id].clone(),
                is_fallback: true,
            },
        }
    }

    pub fn otk_count(&self, id: AccountId) -> usize {
        self.otks.get(&id).map_or(0, Vec::len)
    }

    pub fn send(&mut self, from: AccountId, out: Outgoing) {
        assert_eq!(
            out.kind,
            EnvelopeKind::Control,
            "only control envelopes in these tests"
        );
        self.control
            .entry(out.to)
            .or_default()
            .push_back((from, out.data));
    }

    pub fn send_all(&mut self, from: AccountId, outs: Vec<Outgoing>) {
        for o in outs {
            self.send(from, o);
        }
    }

    pub fn take_control(&mut self, to: AccountId) -> Vec<(AccountId, Vec<u8>)> {
        self.control.remove(&to).map(Vec::from).unwrap_or_default()
    }
}

pub struct Device {
    _dir: tempfile::TempDir,
    pub store: Store,
    pub account: LocalAccount,
    pub friends: Friends,
}

impl Device {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).unwrap();
        let store = Store::open(&dir.path().join("staya.db"), &DbKey::new(key)).unwrap();
        let account = LocalAccount::create(&store).unwrap();
        let friends = Friends::load(&store).unwrap();
        Self {
            _dir: dir,
            store,
            account,
            friends,
        }
    }

    pub fn id(&self) -> AccountId {
        self.account.identity().account_id
    }

    /// Обрабатывает всю очередь; исходящие ответы отправляет через сервер.
    pub fn sync(&mut self, server: &mut FakeServer, now: i64) -> Vec<Handled> {
        let mut results = Vec::new();
        for (from, data) in server.take_control(self.id()) {
            let handled = self
                .friends
                .handle_control(&self.store, &mut self.account, from, &data, now)
                .unwrap();
            server.send_all(self.id(), handled.outgoing.clone());
            results.push(handled);
        }
        results
    }
}
