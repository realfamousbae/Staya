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
    /// Последний пакет позиции для пары (отправитель, получатель) — upsert (§8.1).
    slots: BTreeMap<(AccountId, AccountId), Vec<u8>>,
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
        match out.kind {
            EnvelopeKind::Control => self
                .control
                .entry(out.to)
                .or_default()
                .push_back((from, out.data)),
            EnvelopeKind::Location => {
                self.slots.insert((from, out.to), out.data);
            }
        }
    }

    /// Все слоты позиций для получателя (не удаляются при чтении, как на сервере).
    pub fn slots_for(&self, to: AccountId) -> Vec<(AccountId, Vec<u8>)> {
        self.slots
            .iter()
            .filter(|((_, r), _)| *r == to)
            .map(|((s, _), d)| (*s, d.clone()))
            .collect()
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

    /// Полная выборка по §8.2: сначала очередь, затем слоты позиций.
    pub fn fetch(&mut self, server: &mut FakeServer, now: i64) -> Vec<staya_core::friends::Event> {
        let mut events: Vec<_> = self
            .sync(server, now)
            .into_iter()
            .flat_map(|h| h.events)
            .collect();
        for (from, data) in server.slots_for(self.id()) {
            let h = self
                .friends
                .handle_location(&self.store, from, &data, now)
                .unwrap();
            events.extend(h.events);
        }
        events
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
