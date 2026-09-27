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
        .create_invite(&a.store, &a.account.identity(), InviteMethod::Qr, T0)
        .unwrap();
    let invite = Invite::parse(&invite.to_uri()).unwrap();
    let req = b
        .friends
        .accept_invite(&b.store, &b.account, &invite, &server.claim(a.id()), T0)
        .unwrap();
    server.send(b.id(), req);
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
    let outs = dev
        .friends
        .prepare_location_update(&dev.store, loc, now)
        .unwrap();
    server.send_all(dev.id(), outs);
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
    let outs = alice
        .friends
        .prepare_location_update(&alice.store, Some(HOME), T0 + 5)
        .unwrap();
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
    // Время — момент отправки, иначе повторная заморозка отсеклась бы как повтор.
    assert_eq!(p.timestamp, T0 + 100);
    send(&mut alice, &mut server, None, T0 + 200);
    assert_eq!(
        last_location(&bob.fetch(&mut server, T0 + 201), alice.id())
            .unwrap()
            .timestamp,
        T0 + 200
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
    let outs = alice
        .friends
        .prepare_location_update(&alice.store, Some(at(later)), later)
        .unwrap();
    let kinds: Vec<_> = outs.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, vec![EnvelopeKind::Control, EnvelopeKind::Location]);
    server.send_all(alice.id(), outs);
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
        .handle_location(&bob.store, *from, data, later + 2)
        .unwrap();
    assert!(matches!(h.events[..], [Event::Dropped { .. }]));
}

#[test]
fn packet_arriving_before_its_key_is_held_until_the_key_comes() {
    let (mut alice, mut bob, mut server) = setup();
    let later = T0 + MEGOLM_ROTATION_AGE.as_secs() as i64 + 10;
    let outs = alice
        .friends
        .prepare_location_update(&alice.store, Some(at(later)), later)
        .unwrap();
    let (share, packet): (Vec<_>, Vec<_>) = outs
        .into_iter()
        .partition(|o| o.kind == EnvelopeKind::Control);
    // WebSocket доставил пакет раньше SessionShare.
    let h = bob
        .friends
        .handle_location(&bob.store, alice.id(), &packet[0].data, later)
        .unwrap();
    assert!(matches!(h.events[..], [Event::Dropped { .. }]));
    // Отложенный пакет переживает перезапуск.
    bob.friends = Friends::load(&bob.store).unwrap();
    server.send_all(alice.id(), share);
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
    let outs = alice
        .friends
        .prepare_location_update(&alice.store, Some(HOME), T0 + 5)
        .unwrap();
    // Сервер выдаёт пакет Алисы за пакет Кэрол.
    let h = bob
        .friends
        .handle_location(&bob.store, carol.id(), &outs[0].data, T0 + 6)
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
            .handle_location(&bob.store, stranger, &data, T0)
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
