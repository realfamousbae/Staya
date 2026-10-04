//! API для приложений (`staya_core::api`) — так, как его вызовут Swift и Kotlin:
//! только строки JSON, байты и записи, сеть — на стороне «платформы».

use std::collections::BTreeMap;
use std::sync::Arc;

use staya_core::CoreError;
use staya_core::api::{CoreEvent, InviteMethod, LocationKind, StayaCore};
use staya_core::friends::{Location, Precision};
use staya_core::server::ServerTrust;
use staya_proto::AccountId;
use staya_proto::api::{
    AckRequest, ClaimResponse, EnvelopeKind, EnvelopeStatus, LocationSlot, MailboxResponse,
    PublishKeysRequest, QueuedControl, SendRequest, SendResponse, SignedKey,
};

const T0: i64 = 1_700_000_000;
const HOME: Location = Location {
    lat_e7: 557_558_000,
    lon_e7: 376_173_000,
    accuracy_m: 9,
    timestamp: T0,
};

/// Сервер, который понимает только JSON из `staya_proto::api`.
#[derive(Default)]
struct JsonServer {
    otks: BTreeMap<String, Vec<SignedKey>>,
    control: BTreeMap<String, Vec<QueuedControl>>,
    slots: BTreeMap<(String, String), LocationSlot>,
    next_seq: i64,
    /// Получатели, конверты которым сервер отклоняет (например, переполнена очередь).
    reject_to: Vec<String>,
}

impl JsonServer {
    fn publish(&mut self, who: &str, json: &str) {
        let req: PublishKeysRequest = serde_json::from_str(json).unwrap();
        self.otks
            .entry(who.to_owned())
            .or_default()
            .extend(req.one_time_keys);
    }

    fn claim(&mut self, who: &str) -> String {
        let key = self.otks.get_mut(who).and_then(Vec::pop).expect("an OTK");
        serde_json::to_string(&ClaimResponse {
            key,
            is_fallback: false,
        })
        .unwrap()
    }

    fn send(&mut self, from: &str, json: &str) -> String {
        let req: SendRequest = serde_json::from_str(json).unwrap();
        let mut results = Vec::new();
        for env in req.envelopes {
            let to = env.to.to_b64();
            if self.reject_to.contains(&to) {
                results.push(EnvelopeStatus::Rejected {
                    reason: "queue full".into(),
                });
                continue;
            }
            let from_id = AccountId::from_b64(from).unwrap();
            match env.kind {
                EnvelopeKind::Control => {
                    self.next_seq += 1;
                    let item = QueuedControl {
                        seq: self.next_seq,
                        from: from_id,
                        data: env.data,
                    };
                    self.control.entry(to).or_default().push(item);
                }
                EnvelopeKind::Location => {
                    self.slots.insert(
                        (from.to_owned(), to),
                        LocationSlot {
                            from: from_id,
                            data: env.data,
                        },
                    );
                }
            }
            results.push(EnvelopeStatus::Accepted);
        }
        serde_json::to_string(&SendResponse { results }).unwrap()
    }

    fn mailbox(&self, who: &str) -> String {
        let control = self.control.get(who).cloned().unwrap_or_default();
        let locations = self
            .slots
            .iter()
            .filter(|((_, to), _)| to == who)
            .map(|(_, s)| s.clone())
            .collect();
        serde_json::to_string(&MailboxResponse { control, locations }).unwrap()
    }

    fn ack(&mut self, who: &str, json: &str) {
        let req: AckRequest = serde_json::from_str(json).unwrap();
        if let Some(q) = self.control.get_mut(who) {
            q.retain(|c| !req.seqs.contains(&c.seq));
        }
    }
}

struct App {
    _dir: tempfile::TempDir,
    core: Arc<StayaCore>,
    id: String,
}

impl App {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("staya.db").to_string_lossy().into_owned();
        let core = StayaCore::open(path, vec![42; 32]).unwrap();
        let id = core.identity().unwrap().account_id;
        core.set_server("staya.test".into(), vec![]).unwrap();
        Self {
            _dir: dir,
            core,
            id,
        }
    }

    /// Перезапуск приложения: база закрывается и открывается тем же ключом.
    fn reopen(self) -> Self {
        let Self { _dir, core, id } = self;
        drop(core);
        let path = _dir.path().join("staya.db").to_string_lossy().into_owned();
        let core = StayaCore::open(path, vec![42; 32]).unwrap();
        Self { _dir, core, id }
    }

    /// Как сделает платформа: удаления слотов, затем один POST и `complete_send`.
    fn flush(&self, server: &mut JsonServer) {
        let batch = self.core.pending_sends().unwrap();
        for friend in &batch.delete_slots {
            server.slots.remove(&(self.id.clone(), friend.clone()));
            self.core.mark_slot_deleted(friend.clone()).unwrap();
        }
        if let Some(json) = batch.request_json {
            let response = server.send(&self.id, &json);
            self.core.complete_send(batch.ids, response).unwrap();
        }
    }

    fn sync(&self, server: &mut JsonServer, now: i64) -> Vec<CoreEvent> {
        let processed = self
            .core
            .process_mailbox(server.mailbox(&self.id), now)
            .unwrap();
        if let Some(ack) = processed.ack_json {
            server.ack(&self.id, &ack);
        }
        self.flush(server);
        processed.events
    }
}

fn befriend(alice: &App, bob: &App, server: &mut JsonServer) {
    let keys = alice
        .core
        .keys_to_publish(0, T0)
        .unwrap()
        .expect("fresh keys");
    server.publish(&alice.id, &keys);
    alice.core.mark_keys_published().unwrap();

    let uri = alice.core.create_invite(InviteMethod::Qr, T0).unwrap();
    let info = bob.core.parse_invite(uri.clone()).unwrap();
    assert_eq!(info.account_id, alice.id);
    // Друг подключится к серверу пригласившего (protocol §5.3).
    assert_eq!(info.server, "staya.test");
    assert!(info.server_pins.is_empty());
    bob.core
        .accept_invite(uri, server.claim(&info.account_id), T0)
        .unwrap();
    bob.flush(server);
    assert!(
        alice
            .sync(server, T0 + 1)
            .contains(&CoreEvent::FriendAdded {
                friend: bob.id.clone()
            })
    );
    assert!(bob.sync(server, T0 + 2).contains(&CoreEvent::FriendAdded {
        friend: alice.id.clone()
    }));
}

#[test]
fn full_flow_through_the_api() {
    let mut server = JsonServer::default();
    let (alice, bob) = (App::new(), App::new());
    alice.core.set_profile("Алиса".into(), vec![]).unwrap();
    befriend(&alice, &bob, &mut server);

    let friends = bob.core.list_friends().unwrap();
    assert_eq!(friends.len(), 1);
    assert!(friends[0].active && friends[0].verified);
    assert_eq!(friends[0].nick.as_deref(), Some("Алиса"));
    assert_eq!(friends[0].precision, Precision::Exact);
    assert_eq!(
        alice.core.safety_code(bob.id.clone()).unwrap(),
        bob.core.safety_code(alice.id.clone()).unwrap()
    );

    bob.core
        .prepare_location_update(Some(HOME), T0 + 10)
        .unwrap();
    bob.flush(&mut server);
    let events = alice.sync(&mut server, T0 + 11);
    let CoreEvent::LocationUpdated { friend, location } = &events[0] else {
        panic!("{events:?}")
    };
    assert_eq!(friend, &bob.id);
    assert_eq!(
        (location.kind, location.lat_e7, location.lon_e7),
        (LocationKind::Exact, HOME.lat_e7, HOME.lon_e7)
    );
    assert!(!format!("{location:?}").contains("557558000"));

    // Точность и призрак через API.
    bob.core
        .set_precision(alice.id.clone(), Precision::Hidden)
        .unwrap();
    bob.core
        .prepare_location_update(Some(HOME), T0 + 20)
        .unwrap();
    bob.flush(&mut server);
    let events = alice.sync(&mut server, T0 + 21);
    assert!(
        matches!(&events[0], CoreEvent::LocationUpdated { location, .. } if location.kind == LocationKind::Hidden)
    );

    // Удаление с уведомлением.
    bob.core.remove_friend(alice.id.clone(), true).unwrap();
    bob.flush(&mut server);
    assert!(
        alice
            .sync(&mut server, T0 + 30)
            .contains(&CoreEvent::FriendRemoved {
                friend: bob.id.clone()
            })
    );
    assert!(alice.core.list_friends().unwrap().is_empty());
}

#[test]
fn last_location_survives_restart_and_hidden_replaces_it() {
    let mut server = JsonServer::default();
    let (alice, bob) = (App::new(), App::new());
    befriend(&alice, &bob, &mut server);
    assert_eq!(alice.core.list_friends().unwrap()[0].location, None);

    bob.core
        .prepare_location_update(Some(HOME), T0 + 10)
        .unwrap();
    bob.flush(&mut server);
    alice.sync(&mut server, T0 + 11);

    // После перезапуска пакет из ящика уже не расшифруется, а точка на карте остаётся.
    let alice = alice.reopen();
    let events = alice.sync(&mut server, T0 + 12);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, CoreEvent::LocationUpdated { .. })),
        "{events:?}"
    );
    let location = alice.core.list_friends().unwrap()[0]
        .location
        .expect("stored location");
    assert_eq!(
        (
            location.kind,
            location.lat_e7,
            location.lon_e7,
            location.timestamp
        ),
        (LocationKind::Exact, HOME.lat_e7, HOME.lon_e7, T0)
    );

    // Призрак: прежняя точка не должна оставаться на карте.
    bob.core.set_ghost(true).unwrap();
    bob.core
        .prepare_location_update(Some(HOME), T0 + 20)
        .unwrap();
    bob.flush(&mut server);
    alice.sync(&mut server, T0 + 21);
    let alice = alice.reopen();
    let location = alice.core.list_friends().unwrap()[0]
        .location
        .expect("hidden marker");
    assert_eq!(
        (location.kind, location.lat_e7, location.lon_e7),
        (LocationKind::Hidden, 0, 0)
    );

    // Удаление друга стирает и его позицию.
    alice.core.remove_friend(bob.id.clone(), false).unwrap();
    assert!(alice.core.list_friends().unwrap().is_empty());
}

#[test]
fn rejected_envelopes_are_retired_by_complete_send() {
    let mut server = JsonServer::default();
    let (alice, bob) = (App::new(), App::new());
    befriend(&alice, &bob, &mut server);
    server.reject_to.push(alice.id.clone());
    bob.core
        .prepare_location_update(Some(HOME), T0 + 10)
        .unwrap();
    bob.flush(&mut server);
    assert!(bob.core.pending_sends().unwrap().request_json.is_none());
}

#[test]
fn complete_send_rejects_mismatched_response() {
    let mut server = JsonServer::default();
    let (alice, bob) = (App::new(), App::new());
    befriend(&alice, &bob, &mut server);
    bob.core
        .prepare_location_update(Some(HOME), T0 + 10)
        .unwrap();
    let batch = bob.core.pending_sends().unwrap();
    let empty = serde_json::to_string(&SendResponse { results: vec![] }).unwrap();
    assert!(matches!(
        bob.core.complete_send(batch.ids, empty),
        Err(CoreError::Invalid(_))
    ));
    // Ничего не списано.
    assert!(bob.core.pending_sends().unwrap().request_json.is_some());
}

#[test]
fn state_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("staya.db").to_string_lossy().into_owned();
    let first = StayaCore::open(path.clone(), vec![1; 32]).unwrap();
    let id = first.identity().unwrap();
    first.set_ghost(true).unwrap();
    drop(first);
    let again = StayaCore::open(path, vec![1; 32]).unwrap();
    assert_eq!(again.identity().unwrap(), id);
    assert!(again.sharing().unwrap().ghost);
}

#[test]
fn only_one_open_handle_and_only_the_right_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("staya.db").to_string_lossy().into_owned();
    let first = StayaCore::open(path.clone(), vec![1; 32]).unwrap();
    // Пока база открыта, второй экземпляр (другой процесс, расширение) её не получит.
    assert!(StayaCore::open(path.clone(), vec![1; 32]).is_err());
    drop(first);
    // Чужой ключ не открывает аккаунт.
    assert!(StayaCore::open(path.clone(), vec![2; 32]).is_err());
    // Ключ неверной длины.
    assert!(matches!(
        StayaCore::open(path, vec![1; 16]),
        Err(CoreError::InvalidKey)
    ));
}

fn fresh() -> (tempfile::TempDir, Arc<StayaCore>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("staya.db").to_string_lossy().into_owned();
    (dir, StayaCore::open(path, vec![7; 32]).unwrap())
}

#[test]
fn invite_carries_the_bound_server_and_pins() {
    let (_d, core) = fresh();
    assert!(matches!(
        core.create_invite(InviteMethod::Qr, T0),
        Err(CoreError::NoServer)
    ));
    core.set_server(
        "Self.Hosted.Example:8443".into(),
        vec![vec![5; 32], vec![6; 32]],
    )
    .unwrap();
    let uri = core.create_invite(InviteMethod::Link, T0).unwrap();
    let info = core.parse_invite(uri).unwrap();
    assert_eq!(info.server, "self.hosted.example:8443");
    assert_eq!(info.server_pins, vec![vec![5; 32], vec![6; 32]]);

    let (_d, other) = fresh();
    for bad in [
        ("ok.example", vec![vec![1; 32], vec![2; 32], vec![3; 32]]),
        ("bad host", vec![]),
    ] {
        assert!(matches!(
            other.set_server(bad.0.into(), bad.1),
            Err(CoreError::Proto(_))
        ));
    }
    assert!(matches!(
        other.set_server("ok.example".into(), vec![vec![1; 5]]),
        Err(CoreError::Invalid(_))
    ));
    assert_eq!(other.server().unwrap(), None);
}

#[test]
fn server_can_be_reset_only_before_friends() {
    let (_d, core) = fresh();
    core.set_server("typo.example".into(), vec![]).unwrap();
    core.reset_server().unwrap();
    assert_eq!(core.server().unwrap(), None);
    core.set_server("right.example".into(), vec![]).unwrap();
    // Выданное приглашение привязано к серверу — отвязать уже нельзя.
    core.create_invite(InviteMethod::Qr, T0).unwrap();
    assert!(core.reset_server().is_err());
    assert_eq!(core.server().unwrap().unwrap().host, "right.example");
}

#[test]
fn own_profile_round_trip() {
    let (_d, core) = fresh();
    assert_eq!(core.my_profile().unwrap().nick, "");
    core.set_profile("Лёша".into(), vec![1, 2, 3]).unwrap();
    let p = core.my_profile().unwrap();
    assert_eq!(
        (p.nick.as_str(), p.avatar.as_slice()),
        ("Лёша", &[1u8, 2, 3][..])
    );
    assert!(core.set_profile("x".repeat(65), vec![]).is_err());
    assert!(core.set_profile("ok".into(), vec![0; 8193]).is_err());
    assert_eq!(
        core.my_profile().unwrap().nick,
        "Лёша",
        "failed update keeps the old profile"
    );
}

#[test]
fn account_is_bound_to_one_server() {
    let (_d, core) = fresh();
    let link = "staya://server?v=1&s=a.example";
    let info = core.set_server_from_link(link.into()).unwrap();
    assert_eq!(info.host, "a.example");
    // Тот же сервер — можно; другой — нет, и привязка не меняется.
    core.set_server("a.example".into(), vec![]).unwrap();
    assert!(matches!(
        core.set_server("b.example".into(), vec![]),
        Err(CoreError::ServerMismatch)
    ));
    assert_eq!(core.server().unwrap().unwrap().host, "a.example");

    // Приглашение с чужого сервера не принимается.
    let (_d2, friend) = fresh();
    friend.set_server("b.example".into(), vec![]).unwrap();
    let uri = friend.create_invite(InviteMethod::Qr, T0).unwrap();
    let claim = r#"{"key":{"key":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","signature":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"},"is_fallback":false}"#;
    let r = core.accept_invite(uri, claim.into(), T0);
    assert!(matches!(r, Err(CoreError::ServerMismatch)), "{r:?}");
    // Новый аккаунт может взять сервер из приглашения.
    let (_d3, newbie) = fresh();
    let uri = friend.create_invite(InviteMethod::Qr, T0).unwrap();
    assert_eq!(newbie.set_server_from_link(uri).unwrap().host, "b.example");
}

#[test]
fn tofu_learns_once_and_never_silently_replaces() {
    let (_d, core) = fresh();
    assert!(matches!(
        core.check_server_key(vec![1; 32]),
        Err(CoreError::NoServer)
    ));
    core.set_server("a.example".into(), vec![]).unwrap();
    assert_eq!(
        core.check_server_key(vec![1; 32]).unwrap(),
        ServerTrust::Learned
    );
    assert_eq!(
        core.check_server_key(vec![1; 32]).unwrap(),
        ServerTrust::Trusted
    );
    assert_eq!(
        core.check_server_key(vec![2; 32]).unwrap(),
        ServerTrust::Rejected
    );
    assert_eq!(
        core.server().unwrap().unwrap().learned_pin,
        Some(vec![1; 32])
    );
    assert!(core.check_server_key(vec![1; 5]).is_err());

    // Пережил перезапуск: запомненный ключ — в зашифрованной базе.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("staya.db").to_string_lossy().into_owned();
    {
        let c = StayaCore::open(path.clone(), vec![9; 32]).unwrap();
        c.set_server("a.example".into(), vec![]).unwrap();
        c.check_server_key(vec![3; 32]).unwrap();
    }
    let c = StayaCore::open(path, vec![9; 32]).unwrap();
    assert_eq!(
        c.check_server_key(vec![4; 32]).unwrap(),
        ServerTrust::Rejected
    );

    // Отпечатки из приглашения для того же сервера заменяют TOFU.
    c.set_server("a.example".into(), vec![vec![4; 32]]).unwrap();
    assert_eq!(
        c.check_server_key(vec![4; 32]).unwrap(),
        ServerTrust::Trusted
    );
    assert_eq!(
        c.check_server_key(vec![3; 32]).unwrap(),
        ServerTrust::Rejected
    );
    assert_eq!(c.server().unwrap().unwrap().learned_pin, None);
}

#[test]
fn pinned_server_accepts_primary_and_backup_only() {
    let (_d, core) = fresh();
    core.set_server("a.example".into(), vec![vec![1; 32], vec![2; 32]])
        .unwrap();
    assert_eq!(
        core.check_server_key(vec![1; 32]).unwrap(),
        ServerTrust::Trusted
    );
    assert_eq!(
        core.check_server_key(vec![2; 32]).unwrap(),
        ServerTrust::Trusted
    );
    assert_eq!(
        core.check_server_key(vec![3; 32]).unwrap(),
        ServerTrust::Rejected
    );
    // С отпечатками ключ не запоминается.
    assert_eq!(core.server().unwrap().unwrap().learned_pin, None);
}

#[test]
fn login_helpers_sign_for_the_bound_host_and_keep_the_token() {
    let (_d, core) = fresh();
    core.set_server("Staya.Example:8443".into(), vec![])
        .unwrap();
    assert_eq!(core.session_token(T0).unwrap(), None);
    let challenge = core.auth_challenge_request().unwrap();
    assert!(challenge.contains(&core.identity().unwrap().account_id));

    let nonce = [7u8; 32];
    let resp = format!(r#"{{"nonce":"{}"}}"#, b64(&nonce));
    let verify: serde_json::Value =
        serde_json::from_str(&core.auth_verify_request(resp).unwrap()).unwrap();
    // Подпись — с именем сервера без порта (§4.2).
    let sig = serde_json::from_value::<staya_proto::api::B64>(verify["signature"].clone())
        .unwrap()
        .0;
    let me = core.identity().unwrap();
    let msg = staya_proto::signing::auth(
        "staya.example",
        &nonce,
        &AccountId::from_b64(&me.account_id).unwrap(),
    )
    .unwrap();
    let pk = vodozemac::Ed25519PublicKey::from_slice(&me.sk.try_into().unwrap()).unwrap();
    pk.verify(
        &msg,
        &vodozemac::Ed25519Signature::from_slice(&sig).unwrap(),
    )
    .unwrap();

    let token = [9u8; 32];
    core.complete_login(format!(
        r#"{{"token":"{}","expires_at":{}}}"#,
        b64(&token),
        T0 + 100
    ))
    .unwrap();
    assert_eq!(core.session_token(T0).unwrap(), Some(token.to_vec()));
    assert_eq!(core.session_token(T0 + 100).unwrap(), None, "expired");
    core.clear_session().unwrap();
    assert_eq!(core.session_token(T0).unwrap(), None);
    assert!(
        core.complete_login(r#"{"token":"AAAA","expires_at":1}"#.into())
            .is_err()
    );
}

fn b64(bytes: &[u8]) -> String {
    serde_json::to_value(staya_proto::api::B64(bytes.to_vec()))
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn bad_input_from_the_platform_is_an_error_not_a_panic() {
    let app = App::new();
    assert!(app.core.parse_invite("https://example.com".into()).is_err());
    assert!(app.core.process_mailbox("not json".into(), T0).is_err());
    assert!(
        app.core
            .sign_auth("staya.example".into(), vec![0; 5])
            .is_err()
    );
    assert!(app.core.safety_code("bad id".into()).is_err());
    assert!(matches!(
        app.core.prepare_location_update(None, T0),
        Err(CoreError::MissingLocation)
    ));
}
