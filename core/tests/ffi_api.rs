//! API для приложений (`staya_core::api`) — так, как его вызовут Swift и Kotlin:
//! только строки JSON, байты и записи, сеть — на стороне «платформы».

use std::collections::BTreeMap;
use std::sync::Arc;

use staya_core::CoreError;
use staya_core::api::{CoreEvent, InviteMethod, LocationKind, StayaCore};
use staya_core::friends::{Location, Precision};
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
        Self {
            _dir: dir,
            core,
            id,
        }
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

    let uri = alice
        .core
        .create_invite("staya.test".into(), None, InviteMethod::Qr, T0)
        .unwrap();
    let info = bob.core.parse_invite(uri.clone()).unwrap();
    assert_eq!(info.account_id, alice.id);
    // Друг подключится к серверу пригласившего (protocol §5.3).
    assert_eq!(info.server, "staya.test");
    assert_eq!(info.server_pin, None);
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

#[test]
fn invite_carries_server_and_pin() {
    let app = App::new();
    let uri = app
        .core
        .create_invite(
            "Self.Hosted.Example:8443".into(),
            Some(vec![5; 32]),
            InviteMethod::Link,
            T0,
        )
        .unwrap();
    let info = app.core.parse_invite(uri).unwrap();
    assert_eq!(info.server, "self.hosted.example:8443");
    assert_eq!(info.server_pin, Some(vec![5; 32]));
    assert!(matches!(
        app.core
            .create_invite("bad host".into(), None, InviteMethod::Qr, T0),
        Err(CoreError::Proto(_))
    ));
    assert!(matches!(
        app.core
            .create_invite("ok.example".into(), Some(vec![1; 5]), InviteMethod::Qr, T0),
        Err(CoreError::Invalid(_))
    ));
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
