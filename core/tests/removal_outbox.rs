//! Удаление друга (§9) и исходящая очередь (§8.2).

mod common;

use common::{Device, FakeServer};
use staya_core::friends::{Event, FriendState, Friends, Location};
use staya_proto::AccountId;
use staya_proto::api::EnvelopeKind;
use staya_proto::consts::MEGOLM_ROTATION_AGE;
use staya_proto::invite::InviteMethod;
use staya_proto::location::LocationPayload;

fn staya_test_server() -> staya_proto::invite::ServerRef {
    staya_proto::invite::ServerRef::new("staya.test", vec![]).unwrap()
}

const T0: i64 = 1_700_000_000;
const HOME: Location = Location {
    lat_e7: 557_558_000,
    lon_e7: 376_173_000,
    accuracy_m: 12,
    timestamp: T0,
};

fn at(ts: i64) -> Location {
    Location {
        timestamp: ts,
        lat_e7: HOME.lat_e7 + (ts - T0) as i32,
        ..HOME
    }
}

fn befriend(a: &mut Device, b: &mut Device, server: &mut FakeServer, now: i64) {
    server.publish(a, now);
    let invite = a
        .friends
        .create_invite(
            &a.store,
            &a.account.identity(),
            &staya_test_server(),
            InviteMethod::Qr,
            now,
        )
        .unwrap();
    b.friends
        .accept_invite(&b.store, &b.account, &invite, &server.claim(a.id()), now)
        .unwrap();
    b.flush(server);
    a.sync(server, now);
    b.sync(server, now);
}

fn setup() -> (Device, Device, FakeServer) {
    let mut server = FakeServer::default();
    let (mut alice, mut bob) = (Device::new(), Device::new());
    befriend(&mut alice, &mut bob, &mut server, T0);
    (alice, bob, server)
}

fn send(dev: &mut Device, server: &mut FakeServer, loc: Location, now: i64) {
    dev.friends
        .prepare_location_update(&dev.store, Some(loc), now)
        .unwrap();
    dev.flush(server);
}

fn last_location(events: &[Event], from: AccountId) -> Option<LocationPayload> {
    events.iter().rev().find_map(|e| match e {
        Event::LocationUpdated { friend, payload } if *friend == from => Some(*payload),
        _ => None,
    })
}

#[test]
fn removal_with_notice_revokes_access_on_both_sides() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, at(T0 + 1), T0 + 1);
    send(&mut bob, &mut server, at(T0 + 1), T0 + 1);
    assert!(server.has_slot(alice.id(), bob.id()));

    alice
        .friends
        .remove_friend(&alice.store, &bob.id(), true)
        .unwrap();
    alice.flush(&mut server);
    // Наш последний пакет исчез с сервера сразу.
    assert!(!server.has_slot(alice.id(), bob.id()));
    assert!(alice.friends.list().is_empty());

    // Боб получает Unfriend, удаляет Алису и свой слот у неё.
    let events = bob.fetch(&mut server, T0 + 2);
    assert!(events.contains(&Event::FriendRemoved { friend: alice.id() }));
    assert!(bob.friends.list().is_empty());
    assert!(!server.has_slot(bob.id(), alice.id()));

    // Дальше никто никому ничего не отправляет.
    send(&mut alice, &mut server, at(T0 + 3), T0 + 3);
    send(&mut bob, &mut server, at(T0 + 3), T0 + 3);
    assert!(!server.has_slot(alice.id(), bob.id()));
    assert!(!server.has_slot(bob.id(), alice.id()));
}

#[test]
fn silent_removal_stops_sending_and_ignores_the_old_friend() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, at(T0 + 1), T0 + 1);
    alice
        .friends
        .remove_friend(&alice.store, &bob.id(), false)
        .unwrap();
    alice.flush(&mut server);
    assert!(!server.has_slot(alice.id(), bob.id()));

    // Боб ничего не знает и продолжает слать — Алиса пакеты отбрасывает и не копит.
    assert!(
        bob.fetch(&mut server, T0 + 2)
            .iter()
            .all(|e| !matches!(e, Event::FriendRemoved { .. }))
    );
    send(&mut bob, &mut server, at(T0 + 3), T0 + 3);
    let events = alice.fetch(&mut server, T0 + 4);
    assert!(last_location(&events, bob.id()).is_none());
    assert_eq!(alice.friends.held_packet_count(), 0);
}

#[test]
fn removed_friend_can_be_added_again() {
    let (mut alice, mut bob, mut server) = setup();
    alice
        .friends
        .remove_friend(&alice.store, &bob.id(), true)
        .unwrap();
    alice.flush(&mut server);
    bob.fetch(&mut server, T0 + 1);

    befriend(&mut alice, &mut bob, &mut server, T0 + 10);
    assert_eq!(alice.friends.list()[0].state, FriendState::Active);
    send(&mut alice, &mut server, at(T0 + 11), T0 + 11);
    assert_eq!(
        last_location(&bob.fetch(&mut server, T0 + 12), alice.id())
            .unwrap()
            .timestamp,
        T0 + 11
    );
}

#[test]
fn slot_deletion_survives_restart_and_runs_before_new_packets() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, at(T0 + 1), T0 + 1);
    // Удалили без сети; приложение перезапустилось.
    alice
        .friends
        .remove_friend(&alice.store, &bob.id(), false)
        .unwrap();
    alice.friends = Friends::load(&alice.store).unwrap();
    assert_eq!(alice.friends.pending_sends().delete_slots, vec![bob.id()]);

    // До того как удаление ушло, дружбу восстановили и отправили новую позицию.
    server.publish(&mut alice, T0 + 5);
    let invite = alice
        .friends
        .create_invite(
            &alice.store,
            &alice.account.identity(),
            &staya_test_server(),
            InviteMethod::Qr,
            T0 + 5,
        )
        .unwrap();
    bob.friends
        .remove_friend(&bob.store, &alice.id(), false)
        .unwrap();
    bob.friends
        .accept_invite(
            &bob.store,
            &bob.account,
            &invite,
            &server.claim(alice.id()),
            T0 + 5,
        )
        .unwrap();
    bob.flush(&mut server);
    // Алиса обрабатывает запрос, но ничего ещё не отправила: в очереди и удаление
    // старого слота, и FriendAccept, и новая позиция.
    for (from, data) in server.take_control(alice.id()) {
        alice
            .friends
            .handle_control(&alice.store, &mut alice.account, from, &data, T0 + 6)
            .unwrap();
    }
    alice
        .friends
        .prepare_location_update(&alice.store, Some(at(T0 + 7)), T0 + 7)
        .unwrap();
    let pending = alice.friends.pending_sends();
    assert_eq!(pending.delete_slots, vec![bob.id()]);
    assert!(
        pending
            .envelopes
            .iter()
            .any(|e| e.kind == EnvelopeKind::Location && e.to == bob.id())
    );
    // Одна отправка: удаление идёт раньше конвертов и не стирает новый пакет.
    alice.flush(&mut server);
    assert!(server.has_slot(alice.id(), bob.id()));
    assert!(alice.friends.pending_sends().delete_slots.is_empty());
    bob.sync(&mut server, T0 + 8);
    assert_eq!(
        last_location(&bob.fetch(&mut server, T0 + 9), alice.id())
            .unwrap()
            .timestamp,
        T0 + 7
    );
}

#[test]
fn reply_survives_a_crash_between_processing_and_sending() {
    let mut server = FakeServer::default();
    let (mut alice, mut bob) = (Device::new(), Device::new());
    server.publish(&mut alice, T0);
    let invite = alice
        .friends
        .create_invite(
            &alice.store,
            &alice.account.identity(),
            &staya_test_server(),
            InviteMethod::Qr,
            T0,
        )
        .unwrap();
    bob.friends
        .accept_invite(
            &bob.store,
            &bob.account,
            &invite,
            &server.claim(alice.id()),
            T0,
        )
        .unwrap();
    bob.flush(&mut server);

    // Алиса обработала запрос (очередь сервера подтверждена), но упала до отправки ответа.
    for (from, data) in server.take_control(alice.id()) {
        alice
            .friends
            .handle_control(&alice.store, &mut alice.account, from, &data, T0 + 1)
            .unwrap();
    }
    alice.friends = Friends::load(&alice.store).unwrap();
    let pending = alice.friends.pending_sends().envelopes;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].kind, EnvelopeKind::Control);

    alice.flush(&mut server);
    bob.sync(&mut server, T0 + 2);
    assert_eq!(bob.friends.list()[0].state, FriendState::Active);
}

#[test]
fn failed_rotation_send_does_not_strand_the_friend() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, at(T0 + 1), T0 + 1);
    bob.fetch(&mut server, T0 + 2);

    // Отправка с ротацией сорвалась: ничего не ушло и не отмечено.
    let later = T0 + MEGOLM_ROTATION_AGE.as_secs() as i64 + 10;
    alice
        .friends
        .prepare_location_update(&alice.store, Some(at(later)), later)
        .unwrap();
    // Следующее обновление: в очереди SessionShare первым и только свежая позиция.
    alice
        .friends
        .prepare_location_update(&alice.store, Some(at(later + 60)), later + 60)
        .unwrap();
    let kinds: Vec<_> = alice
        .friends
        .pending_sends()
        .envelopes
        .iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, vec![EnvelopeKind::Control, EnvelopeKind::Location]);

    alice.flush(&mut server);
    let p = last_location(&bob.fetch(&mut server, later + 61), alice.id()).unwrap();
    assert_eq!(p.timestamp, later + 60);
    assert_eq!(bob.friends.held_packet_count(), 0);
}

#[test]
fn unsent_positions_collapse_to_the_latest_per_friend() {
    let mut server = FakeServer::default();
    let mut alice = Device::new();
    let (mut bob, mut carol) = (Device::new(), Device::new());
    befriend(&mut alice, &mut bob, &mut server, T0);
    befriend(&mut alice, &mut carol, &mut server, T0);
    for i in 1..=100 {
        alice
            .friends
            .prepare_location_update(&alice.store, Some(at(T0 + i)), T0 + i)
            .unwrap();
    }
    let pending = alice.friends.pending_sends().envelopes;
    assert_eq!(
        pending
            .iter()
            .filter(|e| e.kind == EnvelopeKind::Location)
            .count(),
        2
    );

    alice.flush(&mut server);
    assert!(alice.friends.pending_sends().envelopes.is_empty());
    for friend in [&mut bob, &mut carol] {
        let p = last_location(&friend.fetch(&mut server, T0 + 101), alice.id()).unwrap();
        assert_eq!(p.timestamp, T0 + 100);
    }
}

#[test]
fn removing_a_friend_discards_their_unsent_envelopes() {
    let (mut alice, bob, _) = setup();
    alice
        .friends
        .prepare_location_update(&alice.store, Some(at(T0 + 1)), T0 + 1)
        .unwrap();
    alice
        .friends
        .remove_friend(&alice.store, &bob.id(), false)
        .unwrap();
    assert!(
        alice
            .friends
            .pending_sends()
            .envelopes
            .iter()
            .all(|e| e.to != bob.id())
    );
}

#[test]
fn rejected_envelopes_are_retired_and_do_not_block_others() {
    let mut server = FakeServer::default();
    let mut alice = Device::new();
    let (mut bob, mut carol) = (Device::new(), Device::new());
    befriend(&mut alice, &mut bob, &mut server, T0);
    befriend(&mut alice, &mut carol, &mut server, T0);
    alice
        .friends
        .prepare_location_update(&alice.store, Some(at(T0 + 1)), T0 + 1)
        .unwrap();

    // Сервер принял конверт Кэрол и окончательно отклонил конверт Боба.
    let pending = alice.friends.pending_sends().envelopes;
    let (to_bob, to_carol): (Vec<_>, Vec<_>) = pending.iter().partition(|e| e.to == bob.id());
    alice
        .friends
        .mark_sent(
            &alice.store,
            &to_carol.iter().map(|e| e.id).collect::<Vec<_>>(),
        )
        .unwrap();
    alice
        .friends
        .mark_rejected(
            &alice.store,
            &to_bob.iter().map(|e| e.id).collect::<Vec<_>>(),
        )
        .unwrap();
    assert!(alice.friends.pending_sends().envelopes.is_empty());

    // Следующие отправки идут обоим как обычно.
    send(&mut alice, &mut server, at(T0 + 2), T0 + 2);
    assert!(last_location(&bob.fetch(&mut server, T0 + 3), alice.id()).is_some());
}
