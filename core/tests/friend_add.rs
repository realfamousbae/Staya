//! Добавление друга по QR и по ссылке (`docs/protocol.md` §5, §6).

mod common;

use common::{Device, FakeServer};
use staya_core::CoreError;
use staya_core::friends::{Event, FriendState, Friends};
use staya_proto::api::B64;
use staya_proto::control::Profile;
use staya_proto::invite::{Invite, InviteMethod};

fn staya_test_server() -> staya_proto::invite::ServerRef {
    staya_proto::invite::ServerRef::new("staya.test", None).unwrap()
}

const T0: i64 = 1_700_000_000;

fn events(handled: &[staya_core::friends::Handled]) -> Vec<Event> {
    handled.iter().flat_map(|h| h.events.clone()).collect()
}

/// A приглашает, B принимает; возвращает устройства после полного обмена.
fn befriend(method: InviteMethod) -> (Device, Device, FakeServer) {
    let mut server = FakeServer::default();
    let (mut alice, mut bob) = (Device::new(), Device::new());
    server.publish(&mut alice, T0);
    server.publish(&mut bob, T0);
    alice
        .friends
        .set_profile(
            &alice.store,
            Profile {
                nick: "Алиса".into(),
                avatar: vec![],
            },
        )
        .unwrap();
    bob.friends
        .set_profile(
            &bob.store,
            Profile {
                nick: "Боб".into(),
                avatar: vec![1, 2, 3],
            },
        )
        .unwrap();

    let invite = alice
        .friends
        .create_invite(
            &alice.store,
            &alice.account.identity(),
            &staya_test_server(),
            method,
            T0,
        )
        .unwrap();
    // Приглашение проходит через текст (QR или ссылку).
    let invite = Invite::parse(&invite.to_uri()).unwrap();
    let claimed = server.claim(invite.account_id);
    bob.friends
        .accept_invite(&bob.store, &bob.account, &invite, &claimed, T0)
        .unwrap();
    bob.flush(&mut server);

    let a = alice.sync(&mut server, T0 + 1);
    assert_eq!(events(&a), vec![Event::FriendAdded { friend: bob.id() }]);
    let b = bob.sync(&mut server, T0 + 2);
    assert_eq!(events(&b), vec![Event::FriendAdded { friend: alice.id() }]);
    (alice, bob, server)
}

#[test]
fn qr_flow_is_verified_and_exchanges_profiles() {
    let (alice, bob, _) = befriend(InviteMethod::Qr);
    let a_view = &alice.friends.list()[0];
    assert_eq!(a_view.account_id, bob.id());
    assert_eq!(a_view.state, FriendState::Active);
    assert!(a_view.verified);
    assert_eq!(a_view.nick.as_deref(), Some("Боб"));
    assert_eq!(a_view.avatar.as_deref(), Some(&[1u8, 2, 3][..]));

    let b_view = &bob.friends.list()[0];
    assert_eq!(b_view.state, FriendState::Active);
    assert!(b_view.verified);
    assert_eq!(b_view.nick.as_deref(), Some("Алиса"));
}

#[test]
fn link_flow_needs_safety_code() {
    let (mut alice, bob, _) = befriend(InviteMethod::Link);
    assert!(!alice.friends.list()[0].verified);
    assert!(!bob.friends.list()[0].verified);

    let a_code = alice
        .friends
        .safety_code(&alice.account.identity(), &bob.id())
        .unwrap();
    let b_code = bob
        .friends
        .safety_code(&bob.account.identity(), &alice.id())
        .unwrap();
    assert_eq!(a_code, b_code);
    assert_eq!(a_code.len(), 60);

    alice
        .friends
        .mark_verified(&alice.store, &bob.id())
        .unwrap();
    assert!(alice.friends.list()[0].verified);
}

#[test]
fn state_survives_restart_and_profile_updates_flow() {
    let (mut alice, mut bob, mut server) = befriend(InviteMethod::Qr);
    // Перезапуск: всё читается из зашифрованной базы.
    alice.friends = Friends::load(&alice.store).unwrap();
    bob.friends = Friends::load(&bob.store).unwrap();

    alice
        .friends
        .set_profile(
            &alice.store,
            Profile {
                nick: "Алиса 2".into(),
                avatar: vec![],
            },
        )
        .unwrap();
    alice.flush(&mut server);
    let b = bob.sync(&mut server, T0 + 10);
    assert_eq!(
        events(&b),
        vec![Event::ProfileUpdated { friend: alice.id() }]
    );
    assert_eq!(bob.friends.list()[0].nick.as_deref(), Some("Алиса 2"));
}

#[test]
fn token_is_single_use() {
    let mut server = FakeServer::default();
    let (mut alice, mut bob, mut carol) = (Device::new(), Device::new(), Device::new());
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
    carol
        .friends
        .accept_invite(
            &carol.store,
            &carol.account,
            &invite,
            &server.claim(alice.id()),
            T0,
        )
        .unwrap();
    carol.flush(&mut server);

    let ev = events(&alice.sync(&mut server, T0 + 1));
    assert_eq!(ev[0], Event::FriendAdded { friend: bob.id() });
    assert_eq!(
        ev[1],
        Event::Dropped {
            reason: "unknown or expired invite token"
        }
    );
    assert_eq!(alice.friends.list().len(), 1);
}

#[test]
fn expired_token_is_rejected() {
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
    // QR живёт 10 минут.
    let ev = events(&alice.sync(&mut server, T0 + 11 * 60));
    assert_eq!(
        ev,
        vec![Event::Dropped {
            reason: "unknown or expired invite token"
        }]
    );
}

#[test]
fn forged_one_time_key_is_rejected() {
    let mut server = FakeServer::default();
    let (mut alice, mut bob, mut mallory) = (Device::new(), Device::new(), Device::new());
    server.publish(&mut alice, T0);
    server.publish(&mut mallory, T0);
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

    // Сервер подсовывает ключ Mallory вместо ключа Алисы.
    let forged = server.claim(mallory.id());
    let err = bob
        .friends
        .accept_invite(&bob.store, &bob.account, &invite, &forged, T0)
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidInvite("key signature")));

    // Подпись от OTK не подходит, если сервер выдаёт ключ как fallback.
    let mut relabeled = server.claim(alice.id());
    relabeled.is_fallback = true;
    assert!(
        bob.friends
            .accept_invite(&bob.store, &bob.account, &invite, &relabeled, T0)
            .is_err()
    );

    // Порченая подпись.
    let mut broken = server.claim(alice.id());
    broken.key.signature = B64(vec![0; 64]);
    assert!(
        bob.friends
            .accept_invite(&bob.store, &bob.account, &invite, &broken, T0)
            .is_err()
    );
}

#[test]
fn server_cannot_spoof_the_sender_of_a_request() {
    let mut server = FakeServer::default();
    let (mut alice, mut bob, carol) = (Device::new(), Device::new(), Device::new());
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
    // Сервер утверждает, что запрос от Кэрол.
    for env in bob.take_pending() {
        server.inject_control(carol.id(), env.to, env.data);
    }
    let ev = events(&alice.sync(&mut server, T0 + 1));
    assert_eq!(
        ev,
        vec![Event::Dropped {
            reason: "sender id mismatch"
        }]
    );
    assert!(alice.friends.list().is_empty());
}

#[test]
fn works_on_fallback_key_when_otks_run_out() {
    let mut server = FakeServer::default();
    let (mut alice, mut bob) = (Device::new(), Device::new());
    server.publish(&mut alice, T0);
    while server.otk_count(alice.id()) > 0 {
        server.claim(alice.id());
    }
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
    let claimed = server.claim(alice.id());
    assert!(claimed.is_fallback);
    bob.friends
        .accept_invite(&bob.store, &bob.account, &invite, &claimed, T0)
        .unwrap();
    bob.flush(&mut server);
    assert_eq!(
        events(&alice.sync(&mut server, T0 + 1)),
        vec![Event::FriendAdded { friend: bob.id() }]
    );
    assert_eq!(
        events(&bob.sync(&mut server, T0 + 2)),
        vec![Event::FriendAdded { friend: alice.id() }]
    );
}

#[test]
fn cannot_accept_own_invite_or_befriend_twice() {
    let (alice, mut bob, mut server) = befriend(InviteMethod::Qr);
    let mut alice = alice;
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
    let claimed = server.claim(alice.id());
    assert!(
        bob.friends
            .accept_invite(&bob.store, &bob.account, &invite, &claimed, T0)
            .is_err()
    );
    let own = alice
        .friends
        .accept_invite(&alice.store, &alice.account, &invite, &claimed, T0);
    assert!(matches!(own, Err(CoreError::InvalidInvite("own invite"))));
}

#[test]
fn garbage_envelopes_are_dropped_not_errors() {
    // Ошибка не дала бы подтвердить сообщение, и оно застряло бы в очереди (§8.2).
    let mut alice = Device::new();
    let stranger = Device::new().id();
    for data in [
        vec![],
        vec![0xAB; 10],
        vec![0xAB; 512],
        vec![0xFF; 1280],
        vec![0; 9472],
    ] {
        let handled = alice
            .friends
            .handle_control(&alice.store, &mut alice.account, stranger, &data, T0)
            .unwrap();
        assert!(matches!(handled.events[..], [Event::Dropped { .. }]));
    }
    assert!(alice.friends.list().is_empty());
    assert!(alice.friends.pending_sends().envelopes.is_empty());
}

#[test]
fn can_accept_a_fresh_invite_after_the_first_one_expired() {
    let mut server = FakeServer::default();
    let (mut alice, mut bob) = (Device::new(), Device::new());
    server.publish(&mut alice, T0);
    let old = alice
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
            &old,
            &server.claim(alice.id()),
            T0,
        )
        .unwrap();
    bob.flush(&mut server);
    // Алиса открыла приложение слишком поздно: токен истёк, запрос отброшен.
    alice.sync(&mut server, T0 + 3600);
    assert_eq!(bob.friends.list()[0].state, FriendState::AwaitingAccept);

    let fresh = alice
        .friends
        .create_invite(
            &alice.store,
            &alice.account.identity(),
            &staya_test_server(),
            InviteMethod::Qr,
            T0 + 3600,
        )
        .unwrap();
    bob.friends
        .accept_invite(
            &bob.store,
            &bob.account,
            &fresh,
            &server.claim(alice.id()),
            T0 + 3600,
        )
        .unwrap();
    bob.flush(&mut server);
    assert_eq!(
        events(&alice.sync(&mut server, T0 + 3601)),
        vec![Event::FriendAdded { friend: bob.id() }]
    );
    assert_eq!(
        events(&bob.sync(&mut server, T0 + 3602)),
        vec![Event::FriendAdded { friend: alice.id() }]
    );
    assert_eq!(bob.friends.list().len(), 1);
}

#[test]
fn redelivered_messages_do_not_duplicate_or_fail() {
    // Сервер доставляет «хотя бы раз»: без ack то же сообщение придёт снова.
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
    // Сервер доставляет один и тот же запрос дважды.
    for env in bob.take_pending() {
        server.deliver(bob.id(), &env);
        server.deliver(bob.id(), &env);
    }
    let ev = events(&alice.sync(&mut server, T0 + 1));
    assert_eq!(ev[0], Event::FriendAdded { friend: bob.id() });
    assert!(matches!(ev[1], Event::Dropped { .. }));
    assert_eq!(alice.friends.list().len(), 1);

    // Повтор FriendAccept у Боба тоже безвреден.
    let accept = server.take_control(bob.id());
    for (from, data) in accept.iter().chain(accept.iter()) {
        bob.friends
            .handle_control(&bob.store, &mut bob.account, *from, data, T0 + 2)
            .unwrap();
    }
    assert_eq!(bob.friends.list()[0].state, FriendState::Active);
}

#[test]
fn simultaneous_mutual_invites_do_not_wedge() {
    let mut server = FakeServer::default();
    let (mut alice, mut bob) = (Device::new(), Device::new());
    server.publish(&mut alice, T0);
    server.publish(&mut bob, T0);
    let a_inv = alice
        .friends
        .create_invite(
            &alice.store,
            &alice.account.identity(),
            &staya_test_server(),
            InviteMethod::Qr,
            T0,
        )
        .unwrap();
    let b_inv = bob
        .friends
        .create_invite(
            &bob.store,
            &bob.account.identity(),
            &staya_test_server(),
            InviteMethod::Qr,
            T0,
        )
        .unwrap();

    // Оба сканируют QR друг друга до того, как получили что-либо.
    bob.friends
        .accept_invite(
            &bob.store,
            &bob.account,
            &a_inv,
            &server.claim(alice.id()),
            T0,
        )
        .unwrap();
    bob.flush(&mut server);
    alice
        .friends
        .accept_invite(
            &alice.store,
            &alice.account,
            &b_inv,
            &server.claim(bob.id()),
            T0,
        )
        .unwrap();
    alice.flush(&mut server);

    for i in 0..3 {
        alice.sync(&mut server, T0 + 1 + i);
        bob.sync(&mut server, T0 + 1 + i);
    }
    assert_eq!(alice.friends.list()[0].state, FriendState::Active);
    assert_eq!(bob.friends.list()[0].state, FriendState::Active);

    // Канал работает в обе стороны.
    alice
        .friends
        .set_profile(
            &alice.store,
            Profile {
                nick: "A".into(),
                avatar: vec![],
            },
        )
        .unwrap();
    alice.flush(&mut server);
    assert_eq!(
        events(&bob.sync(&mut server, T0 + 10)),
        vec![Event::ProfileUpdated { friend: alice.id() }]
    );
    bob.friends
        .set_profile(
            &bob.store,
            Profile {
                nick: "B".into(),
                avatar: vec![],
            },
        )
        .unwrap();
    bob.flush(&mut server);
    assert_eq!(
        events(&alice.sync(&mut server, T0 + 11)),
        vec![Event::ProfileUpdated { friend: bob.id() }]
    );
}
