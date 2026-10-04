//! Цели fuzzing (задача 2.7).
//!
//! Тела целей живут здесь, чтобы их гоняли и обычные тесты на стабильном Rust
//! (proptest ниже, при каждом PR), и libFuzzer в `core/fuzz` (nightly, в CI по
//! расписанию). Модуль собирается только в тестах и с feature `fuzzing`.
//!
//! Главный инвариант (protocol §8.2): всё, что присылают сервер или собеседник,
//! даёт `Ok`, а не ошибку и не панику — иначе сообщение застрянет в очереди.

use staya_proto::AccountId;
use staya_proto::api::{ClaimResponse, EnvelopeKind};
use staya_proto::control::ControlMessage;
use staya_proto::envelope::{ControlEnvelope, LocationEnvelope};
use staya_proto::invite::{Invite, InviteMethod, ServerRef};
use staya_proto::location::LocationPayload;

use crate::account::LocalAccount;
use crate::friends::{Event, Friends, Location};
use crate::store::{DbKey, Store};

const T0: i64 = 1_700_000_000;
const HOME: Location = Location {
    lat_e7: 557_558_000,
    lon_e7: 376_173_000,
    accuracy_m: 10,
    timestamp: T0,
};

struct Device {
    store: Store,
    account: LocalAccount,
    friends: Friends,
}

impl Device {
    fn new() -> Self {
        let store = Store::open_in_memory(&DbKey::new([7; 32])).expect("in-memory store");
        let account = LocalAccount::create(&store).expect("account");
        let friends = Friends::load(&store).expect("friends");
        Self {
            store,
            account,
            friends,
        }
    }

    fn id(&self) -> AccountId {
        self.account.identity().account_id
    }

    /// Доставляет всю исходящую очередь `self` устройству `to`.
    fn deliver_to(&mut self, to: &mut Device) {
        let pending = self.friends.pending_sends().envelopes;
        for env in &pending {
            match env.kind {
                EnvelopeKind::Control => {
                    to.friends
                        .handle_control(&to.store, &mut to.account, self.id(), &env.data, T0)
                        .expect("setup");
                }
                EnvelopeKind::Location => {
                    to.friends
                        .handle_location(&to.store, self.id(), &env.data)
                        .expect("setup");
                }
            }
        }
        let ids: Vec<u64> = pending.iter().map(|e| e.id).collect();
        self.friends.mark_sent(&self.store, &ids).expect("setup");
    }
}

/// Алиса и Боб — друзья; у Боба в очереди нет ничего неотправленного.
fn befriended_pair() -> (Device, Device) {
    let (mut alice, mut bob) = (Device::new(), Device::new());
    let keys = alice
        .account
        .keys_to_publish(&alice.store, 0, T0)
        .expect("keys")
        .expect("fresh keys");
    alice
        .account
        .mark_keys_published(&alice.store)
        .expect("publish");
    let claimed = ClaimResponse {
        key: keys.one_time_keys[0].clone(),
        is_fallback: false,
    };
    let invite = alice
        .friends
        .create_invite(
            &alice.store,
            &alice.account.identity(),
            &ServerRef::new("staya.test", vec![]).expect("server"),
            InviteMethod::Qr,
            T0,
        )
        .expect("invite");
    bob.friends
        .accept_invite(&bob.store, &bob.account, &invite, &claimed, T0)
        .expect("accept");
    bob.deliver_to(&mut alice);
    alice.deliver_to(&mut bob);
    (alice, bob)
}

/// Подсказка отправителя: друг, незнакомец или произвольный ID.
fn hint(byte: u8, friend: AccountId, data: &[u8]) -> AccountId {
    match byte % 3 {
        0 => friend,
        1 => AccountId([0xEE; 16]),
        _ => {
            let mut id = [0u8; 16];
            for (slot, b) in id.iter_mut().zip(data) {
                *slot = *b;
            }
            AccountId(id)
        }
    }
}

/// Искажает настоящий конверт байтами входа (XOR), сохраняя размер.
fn mutate(mut real: Vec<u8>, data: &[u8]) -> Vec<u8> {
    for (slot, b) in real.iter_mut().zip(data.iter().cycle()) {
        *slot ^= b;
    }
    real
}

/// Приводит байты к допустимому размеру конверта, чтобы пройти проверку размера.
fn sized(data: &[u8], len: usize) -> Vec<u8> {
    let mut v = data.to_vec();
    v.resize(len, 0);
    v
}

/// Управляющий конверт от сервера или собеседника: `data[0]` — форма входа,
/// `data[1]` — подсказка отправителя, остальное — тело.
pub fn handle_control(data: &[u8]) -> Vec<Event> {
    let (Some(&mode), Some(&hint_byte)) = (data.first(), data.get(1)) else {
        return vec![];
    };
    let body = &data[2..];
    let (mut alice, mut bob) = befriended_pair();
    let friends_before = alice.friends.list().len();

    let envelope = match mode % 4 {
        0 => body.to_vec(),
        1 => sized(body, 512),
        2 => sized(body, 1280),
        _ => {
            // Настоящее сообщение Боба (смена профиля) с искажениями.
            bob.friends
                .set_profile(
                    &bob.store,
                    staya_proto::control::Profile {
                        nick: "b".into(),
                        avatar: vec![],
                    },
                )
                .expect("profile");
            let real = bob
                .friends
                .pending_sends()
                .envelopes
                .pop()
                .expect("queued profile")
                .data;
            mutate(real, body)
        }
    };
    let from = hint(hint_byte, bob.id(), body);
    let handled = alice
        .friends
        .handle_control(&alice.store, &mut alice.account, from, &envelope, T0 + 1)
        .expect("peer input must never be an error");

    // Чужой ввод не добавляет друзей и не порождает ответов, кроме допустимых.
    assert!(alice.friends.list().len() <= friends_before);
    for event in &handled.events {
        assert!(matches!(
            event,
            Event::Dropped { .. } | Event::ProfileUpdated { .. } | Event::FriendRemoved { .. }
        ));
    }
    handled.events
}

/// Конверт позиции: `data[0]` — форма входа, `data[1]` — подсказка, остальное — тело.
pub fn handle_location(data: &[u8]) -> Vec<Event> {
    let (Some(&mode), Some(&hint_byte)) = (data.first(), data.get(1)) else {
        return vec![];
    };
    let body = &data[2..];
    let (mut alice, mut bob) = befriended_pair();

    let envelope = match mode % 3 {
        0 => body.to_vec(),
        1 => sized(body, 160),
        _ => {
            // Настоящий пакет Боба с искажениями.
            bob.friends
                .prepare_location_update(&bob.store, Some(HOME), T0 + 1)
                .expect("update");
            let real = bob
                .friends
                .pending_sends()
                .envelopes
                .into_iter()
                .find(|e| e.kind == EnvelopeKind::Location)
                .expect("queued location")
                .data;
            mutate(real, body)
        }
    };
    let from = hint(hint_byte, bob.id(), body);
    let handled = alice
        .friends
        .handle_location(&alice.store, from, &envelope)
        .expect("peer input must never be an error");

    // Позиция может прийти только от настоящего друга — Боба.
    for event in &handled.events {
        match event {
            Event::LocationUpdated { friend, .. } => assert_eq!(*friend, bob.id()),
            Event::Dropped { .. } => {}
            other => panic!("unexpected event {other:?}"),
        }
    }
    // Отложить можно не больше одного пакета на друга.
    assert!(alice.friends.held_packet_count() <= 1);
    handled.events
}

/// Разбор всего, что приходит из сети или из QR, без состояния.
pub fn decode(data: &[u8]) {
    let _ = LocationPayload::decode(data);
    let _ = ControlMessage::decode(data);
    if let Ok(env) = LocationEnvelope::from_bytes(data) {
        let _ = env.open();
    }
    if let Ok(env) = ControlEnvelope::from_bytes(data) {
        let _ = env.open();
    }
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = Invite::parse(text);
        let _ = ServerRef::parse_link(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        // Состояние поднимается заново на каждый вход, поэтому случаев немного;
        // длинные прогоны — в libFuzzer (core/fuzz).
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn control_input_never_errors(data in proptest::collection::vec(any::<u8>(), 0..1400)) {
            handle_control(&data);
        }

        #[test]
        fn location_input_never_errors(data in proptest::collection::vec(any::<u8>(), 0..200)) {
            handle_location(&data);
        }

        #[test]
        fn decoders_never_panic(data in proptest::collection::vec(any::<u8>(), 0..1400)) {
            decode(&data);
        }
    }

    #[test]
    fn unmutated_real_envelopes_are_accepted() {
        // Нулевая маска оставляет настоящие конверты нетронутыми: цели доходят
        // до успешной расшифровки, а не только до отказов.
        let mut control = vec![3u8, 0];
        control.extend([0u8; 64]);
        assert!(matches!(
            handle_control(&control)[..],
            [Event::ProfileUpdated { .. }]
        ));
        let mut location = vec![2u8, 0];
        location.extend([0u8; 64]);
        assert!(matches!(
            handle_location(&location)[..],
            [Event::LocationUpdated { .. }]
        ));

        // Один изменённый байт шифротекста — уже отказ, а не ошибка.
        let mut flipped = vec![2u8, 0];
        flipped.extend([0u8; 40]);
        flipped.push(1);
        assert!(matches!(
            handle_location(&flipped)[..],
            [Event::Dropped { .. }]
        ));
    }
}
