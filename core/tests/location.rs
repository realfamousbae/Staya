//! Канал позиций на Megolm (`docs/protocol.md` §7).

mod common;

use common::{Device, FakeServer};
use staya_core::CoreError;
use staya_core::friends::{Event, Friends, Location, Precision};
use staya_proto::AccountId;
use staya_proto::api::EnvelopeKind;
use staya_proto::consts::{LOCATION_ENVELOPE_LEN, MEGOLM_ROTATION_AGE};
use staya_proto::invite::{Invite, InviteMethod};
use staya_proto::location::{LocationKind, LocationPayload, snap_to_grid};

fn staya_test_server() -> staya_proto::invite::ServerRef {
    staya_proto::invite::ServerRef::new("staya.test", None).unwrap()
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
        lat_e7: HOME.lat_e7 + (ts - T0) as i32 * 10,
        ..HOME
    }
}

fn befriend(a: &mut Device, b: &mut Device, server: &mut FakeServer) {
    server.publish(a, T0);
    let invite = a
        .friends
        .create_invite(
            &a.store,
            &a.account.identity(),
            &staya_test_server(),
            InviteMethod::Qr,
            T0,
        )
        .unwrap();
    let invite = Invite::parse(&invite.to_uri()).unwrap();
    b.friends
        .accept_invite(&b.store, &b.account, &invite, &server.claim(a.id()), T0)
        .unwrap();
    b.flush(server);
    a.sync(server, T0);
    b.sync(server, T0);
}

fn setup() -> (Device, Device, FakeServer) {
    let mut server = FakeServer::default();
    let (mut alice, mut bob) = (Device::new(), Device::new());
    befriend(&mut alice, &mut bob, &mut server);
    (alice, bob, server)
}

fn send(dev: &mut Device, server: &mut FakeServer, loc: Option<Location>, now: i64) {
    dev.friends
        .prepare_location_update(&dev.store, loc, now)
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
fn exact_location_reaches_friend() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, Some(HOME), T0 + 5);
    let p = last_location(&bob.fetch(&mut server, T0 + 6), alice.id()).unwrap();
    assert_eq!(p.kind, LocationKind::Exact);
    assert_eq!(
        (p.lat_e7, p.lon_e7, p.accuracy_m, p.timestamp),
        (HOME.lat_e7, HOME.lon_e7, 12, T0)
    );
}

#[test]
fn approx_snaps_to_grid_and_hidden_has_no_coordinates() {
    let (mut alice, mut bob, mut server) = setup();
    alice
        .friends
        .set_precision(&alice.store, &bob.id(), Precision::Approx)
        .unwrap();
    send(&mut alice, &mut server, Some(HOME), T0 + 5);
    let p = last_location(&bob.fetch(&mut server, T0 + 6), alice.id()).unwrap();
    let (lat, lon, acc) = snap_to_grid(HOME.lat_e7, HOME.lon_e7);
    assert_eq!(
        (p.kind, p.lat_e7, p.lon_e7, p.accuracy_m),
        (LocationKind::Approx, lat, lon, acc)
    );

    alice
        .friends
        .set_precision(&alice.store, &bob.id(), Precision::Hidden)
        .unwrap();
    send(&mut alice, &mut server, Some(at(T0 + 10)), T0 + 10);
    let p = last_location(&bob.fetch(&mut server, T0 + 11), alice.id()).unwrap();
    assert_eq!((p.kind, p.lat_e7, p.lon_e7), (LocationKind::Hidden, 0, 0));
}

#[test]
fn modes_are_indistinguishable_on_the_wire() {
    // Три друга с разной точностью получают конверты одного размера в одной отправке.
    let mut server = FakeServer::default();
    let mut alice = Device::new();
    let mut friends: Vec<Device> = (0..3).map(|_| Device::new()).collect();
    for f in &mut friends {
        befriend(&mut alice, f, &mut server);
    }
    alice
        .friends
        .set_precision(&alice.store, &friends[1].id(), Precision::Approx)
        .unwrap();
    alice
        .friends
        .set_precision(&alice.store, &friends[2].id(), Precision::Hidden)
        .unwrap();
    alice
        .friends
        .prepare_location_update(&alice.store, Some(HOME), T0 + 5)
        .unwrap();
    let outs = alice.take_pending();
    let locs: Vec<_> = outs
        .iter()
        .filter(|o| o.kind == EnvelopeKind::Location)
        .collect();
    assert_eq!(locs.len(), 3);
    assert!(locs.iter().all(|o| o.data.len() == LOCATION_ENVELOPE_LEN));
}

#[test]
fn ghost_and_freeze() {
    let (mut alice, mut bob, mut server) = setup();
    alice.friends.set_ghost(&alice.store, true).unwrap();
    // В режиме призрака позиция не нужна.
    send(&mut alice, &mut server, None, T0 + 5);
    let p = last_location(&bob.fetch(&mut server, T0 + 6), alice.id()).unwrap();
    assert_eq!(p.kind, LocationKind::Hidden);

    alice.friends.set_ghost(&alice.store, false).unwrap();
    alice.friends.set_frozen(&alice.store, Some(HOME)).unwrap();
    send(&mut alice, &mut server, Some(at(T0 + 100)), T0 + 100);
    let p = last_location(&bob.fetch(&mut server, T0 + 101), alice.id()).unwrap();
    assert_eq!(
        (p.kind, p.lat_e7, p.lon_e7),
        (LocationKind::Frozen, HOME.lat_e7, HOME.lon_e7)
    );
    // Заморозка показывает исходное время замера; повторные пакеты всё равно принимаются.
    assert_eq!(p.timestamp, HOME.timestamp);
    send(&mut alice, &mut server, None, T0 + 200);
    assert_eq!(
        last_location(&bob.fetch(&mut server, T0 + 201), alice.id())
            .unwrap()
            .kind,
        LocationKind::Frozen
    );

    alice.friends.set_frozen(&alice.store, None).unwrap();
    let err = alice
        .friends
        .prepare_location_update(&alice.store, None, T0 + 300)
        .unwrap_err();
    assert!(matches!(err, CoreError::MissingLocation));
}

#[test]
fn only_the_latest_packet_after_5000_updates_still_decrypts() {
    // Главный сценарий, ради которого выбран Megolm (§2.1): сервер хранит только
    // последний пакет, получатель пропустил тысячи обновлений и несколько ротаций.
    let (mut alice, mut bob, mut server) = setup();
    for i in 1..=5000 {
        send(&mut alice, &mut server, Some(at(T0 + i)), T0 + i);
    }
    let events = bob.fetch(&mut server, T0 + 5001);
    let p = last_location(&events, alice.id()).unwrap();
    assert_eq!(p.timestamp, T0 + 5000);
    assert_eq!(p.lat_e7, at(T0 + 5000).lat_e7);
}

#[test]
fn rotation_by_time_shares_key_before_packet() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, Some(at(T0 + 1)), T0 + 1);
    bob.fetch(&mut server, T0 + 2);
    let later = T0 + MEGOLM_ROTATION_AGE.as_secs() as i64 + 10;
    alice
        .friends
        .prepare_location_update(&alice.store, Some(at(later)), later)
        .unwrap();
    let outs = alice.take_pending();
    let kinds: Vec<_> = outs.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, vec![EnvelopeKind::Control, EnvelopeKind::Location]);
    for env in &outs {
        server.deliver(alice.id(), env);
    }
    assert_eq!(
        last_location(&bob.fetch(&mut server, later + 1), alice.id())
            .unwrap()
            .timestamp,
        later
    );
}

#[test]
fn replayed_and_rolled_back_packets_are_rejected() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, Some(at(T0 + 1)), T0 + 1);
    let old_packet = server.slots_for(bob.id());
    assert!(last_location(&bob.fetch(&mut server, T0 + 2), alice.id()).is_some());

    // Тот же пакет ещё раз — повтор.
    let ev = bob.fetch(&mut server, T0 + 3);
    assert!(last_location(&ev, alice.id()).is_none());

    // Ротация, новый пакет принят; затем сервер подсовывает пакет старой сессии.
    let later = T0 + MEGOLM_ROTATION_AGE.as_secs() as i64 + 10;
    send(&mut alice, &mut server, Some(at(later)), later);
    assert!(last_location(&bob.fetch(&mut server, later + 1), alice.id()).is_some());
    let (from, data) = &old_packet[0];
    let h = bob
        .friends
        .handle_location(&bob.store, *from, data)
        .unwrap();
    assert!(matches!(h.events[..], [Event::Dropped { .. }]));
}

#[test]
fn packet_arriving_before_its_key_is_held_until_the_key_comes() {
    let (mut alice, mut bob, mut server) = setup();
    let later = T0 + MEGOLM_ROTATION_AGE.as_secs() as i64 + 10;
    alice
        .friends
        .prepare_location_update(&alice.store, Some(at(later)), later)
        .unwrap();
    let outs = alice.take_pending();
    let (share, packet): (Vec<_>, Vec<_>) = outs
        .into_iter()
        .partition(|o| o.kind == EnvelopeKind::Control);
    // WebSocket доставил пакет раньше SessionShare.
    let h = bob
        .friends
        .handle_location(&bob.store, alice.id(), &packet[0].data)
        .unwrap();
    assert!(matches!(h.events[..], [Event::Dropped { .. }]));
    // Отложенный пакет переживает перезапуск.
    bob.friends = Friends::load(&bob.store).unwrap();
    for env in &share {
        server.deliver(alice.id(), env);
    }
    let events: Vec<_> = bob
        .sync(&mut server, later + 1)
        .into_iter()
        .flat_map(|h| h.events)
        .collect();
    assert_eq!(last_location(&events, alice.id()).unwrap().timestamp, later);
}

#[test]
fn packet_is_attributed_by_session_not_by_server_label() {
    let mut server = FakeServer::default();
    let (mut alice, mut bob, mut carol) = (Device::new(), Device::new(), Device::new());
    befriend(&mut alice, &mut bob, &mut server);
    befriend(&mut carol, &mut bob, &mut server);
    alice
        .friends
        .prepare_location_update(&alice.store, Some(HOME), T0 + 5)
        .unwrap();
    let outs = alice.take_pending();
    // Сервер выдаёт пакет Алисы за пакет Кэрол.
    let h = bob
        .friends
        .handle_location(&bob.store, carol.id(), &outs[0].data)
        .unwrap();
    assert!(matches!(h.events[0], Event::LocationUpdated { friend, .. } if friend == alice.id()));
}

#[test]
fn garbage_location_envelopes_are_dropped() {
    let (_, mut bob, _) = setup();
    let stranger = Device::new().id();
    for data in [vec![], vec![0; 160], vec![0xFF; 160], vec![1; 512]] {
        let h = bob
            .friends
            .handle_location(&bob.store, stranger, &data)
            .unwrap();
        assert!(matches!(h.events[..], [Event::Dropped { .. }]));
    }
}

#[test]
fn debug_output_never_contains_coordinates() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, Some(HOME), T0 + 5);
    let events = bob.fetch(&mut server, T0 + 6);
    let dump = format!("{events:?} {HOME:?}");
    assert!(
        !dump.contains("557558000") && !dump.contains("376173000"),
        "{dump}"
    );
}

#[test]
fn refetching_the_same_slot_is_a_duplicate_and_is_not_held() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, Some(HOME), T0 + 5);
    assert!(last_location(&bob.fetch(&mut server, T0 + 6), alice.id()).is_some());
    // Сервер отдаёт все слоты при каждой выборке: второй раз — тот же пакет.
    let ev = bob.fetch(&mut server, T0 + 7);
    assert_eq!(
        ev,
        vec![Event::Dropped {
            reason: "location already seen"
        }]
    );
    assert_eq!(bob.friends.held_packet_count(), 0);
}

#[test]
fn packets_under_unknown_hints_are_not_held() {
    let (mut alice, _, _) = setup();
    let mut stranger = Device::new();
    let mut server = FakeServer::default();
    let mut other = Device::new();
    befriend(&mut stranger, &mut other, &mut server);
    stranger
        .friends
        .prepare_location_update(&stranger.store, Some(HOME), T0)
        .unwrap();
    let outs = stranger.take_pending();
    for i in 0..10u8 {
        let hint = AccountId([i; 16]);
        alice
            .friends
            .handle_location(&alice.store, hint, &outs[0].data)
            .unwrap();
    }
    assert_eq!(alice.friends.held_packet_count(), 0);
}

/// После снятия режима друг сразу видит позицию, даже если замер старше
/// последнего пакета (кэшированная точка при SLC).
fn assert_next_exact_is_shown(
    alice: &mut Device,
    bob: &mut Device,
    server: &mut FakeServer,
    now: i64,
) {
    let cached_fix = Location {
        timestamp: T0 + 1,
        ..HOME
    };
    send(alice, server, Some(cached_fix), now);
    let p = last_location(&bob.fetch(server, now + 1), alice.id()).expect("location shown");
    assert_eq!((p.kind, p.timestamp), (LocationKind::Exact, T0 + 1));
}

#[test]
fn leaving_freeze_ghost_or_hidden_shows_the_next_fix_immediately() {
    let (mut alice, mut bob, mut server) = setup();

    alice
        .friends
        .set_frozen(
            &alice.store,
            Some(Location {
                timestamp: T0 + 50,
                ..HOME
            }),
        )
        .unwrap();
    send(&mut alice, &mut server, None, T0 + 100);
    bob.fetch(&mut server, T0 + 101);
    alice.friends.set_frozen(&alice.store, None).unwrap();
    assert_next_exact_is_shown(&mut alice, &mut bob, &mut server, T0 + 102);

    alice.friends.set_ghost(&alice.store, true).unwrap();
    send(&mut alice, &mut server, None, T0 + 200);
    bob.fetch(&mut server, T0 + 201);
    alice.friends.set_ghost(&alice.store, false).unwrap();
    assert_next_exact_is_shown(&mut alice, &mut bob, &mut server, T0 + 202);

    alice
        .friends
        .set_precision(&alice.store, &bob.id(), Precision::Hidden)
        .unwrap();
    send(&mut alice, &mut server, Some(HOME), T0 + 300);
    bob.fetch(&mut server, T0 + 301);
    alice
        .friends
        .set_precision(&alice.store, &bob.id(), Precision::Exact)
        .unwrap();
    assert_next_exact_is_shown(&mut alice, &mut bob, &mut server, T0 + 302);
}

#[test]
fn two_sends_in_the_same_second_keep_the_later_state() {
    let (mut alice, mut bob, mut server) = setup();
    send(&mut alice, &mut server, Some(HOME), T0 + 5);
    bob.fetch(&mut server, T0 + 5);
    alice.friends.set_ghost(&alice.store, true).unwrap();
    send(&mut alice, &mut server, None, T0 + 5);
    let p = last_location(&bob.fetch(&mut server, T0 + 5), alice.id()).unwrap();
    assert_eq!(p.kind, LocationKind::Hidden);
}

#[test]
fn packet_before_friend_accept_is_held_and_then_shown() {
    // Алиса (пригласившая) сразу шлёт позицию; у Боба дружба ещё не подтверждена.
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
    alice.sync(&mut server, T0 + 1);
    alice
        .friends
        .prepare_location_update(&alice.store, Some(HOME), T0 + 2)
        .unwrap();
    let outs = alice.take_pending();
    let packet = outs
        .into_iter()
        .find(|o| o.kind == EnvelopeKind::Location)
        .unwrap();
    // WebSocket доставил пакет раньше FriendAccept.
    bob.friends
        .handle_location(&bob.store, alice.id(), &packet.data)
        .unwrap();
    assert_eq!(bob.friends.held_packet_count(), 1);
    let events: Vec<_> = bob
        .sync(&mut server, T0 + 3)
        .into_iter()
        .flat_map(|h| h.events)
        .collect();
    assert!(last_location(&events, alice.id()).is_some());
    assert_eq!(bob.friends.held_packet_count(), 0);
}
