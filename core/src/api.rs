//! API ядра для приложений (UniFFI, задача 2.8).
//!
//! Один объект [`StayaCore`] держит хранилище, аккаунт и друзей за одним
//! мьютексом: платформы вызывают ядро одновременно из колбэка геолокации,
//! WebSocket и UI, а цепочка «`pending_sends` → отправка → `complete_send`»
//! должна идти последовательно. Ядро не ходит в сеть: оно отдаёт тела HTTP-запросов
//! в JSON (схемы `staya_proto::api`) и разбирает ответы сервера.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use staya_proto::AccountId;
use staya_proto::api::{
    AckRequest, B64, ChallengeRequest, ChallengeResponse, ClaimResponse, EnvelopeStatus,
    MailboxResponse, OutgoingEnvelope, SendRequest, SendResponse, VerifyRequest, VerifyResponse,
    WsEvent,
};
use staya_proto::control::Profile;
use staya_proto::invite::{Invite, InviteMethod as ProtoInviteMethod, ServerRef};
use staya_proto::location::{LocationKind as ProtoLocationKind, LocationPayload};

use crate::CoreError;
use crate::account::LocalAccount;
use crate::friends::{Event, FriendState, Friends, Location, Precision, Sharing};
use crate::server::{ServerBinding, ServerTrust};
use crate::store::{DbKey, Store};

struct Inner {
    store: Store,
    account: LocalAccount,
    friends: Friends,
    server: Option<ServerBinding>,
}

/// Свой профиль, который друзья получают по E2E (§6).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MyProfile {
    pub nick: String,
    pub avatar: Vec<u8>,
}

/// Сервер аккаунта (protocol §5.3).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ServerInfo {
    /// `host` или `host:port` — куда подключаться и имя для проверки TLS.
    pub host: String,
    /// Отпечатки SHA-256 SPKI из приглашения или ссылки на сервер.
    pub pins: Vec<Vec<u8>>,
    /// Ключ, запомненный при первом подключении (когда отпечатков нет).
    pub learned_pin: Option<Vec<u8>>,
}

impl ServerInfo {
    fn from_binding(b: &ServerBinding) -> Self {
        Self {
            host: b.server().host.clone(),
            pins: b.server().pins.iter().map(|p| p.to_vec()).collect(),
            learned_pin: b.learned().map(|p| p.to_vec()),
        }
    }
}

/// Ядро Staya на устройстве.
#[derive(uniffi::Object)]
pub struct StayaCore {
    inner: Mutex<Inner>,
}

/// Способ передачи приглашения (§5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum InviteMethod {
    /// QR при встрече — друг сразу «проверен».
    Qr,
    /// Ссылка — нужно сверить код безопасности.
    Link,
}

impl From<InviteMethod> for ProtoInviteMethod {
    fn from(m: InviteMethod) -> Self {
        match m {
            InviteMethod::Qr => Self::Qr,
            InviteMethod::Link => Self::Link,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct IdentityInfo {
    /// base64url, как в API и приглашениях.
    pub account_id: String,
    pub ik: Vec<u8>,
    pub sk: Vec<u8>,
}

/// Кому и чей одноразовый ключ запросить перед `accept_invite`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct InviteInfo {
    /// Сервер пригласившего (`host` или `host:port`): подключаться к нему (protocol §5.3).
    pub server: String,
    /// Отпечатки ключей TLS сервера (SHA-256 SPKI): пусто, один или два (§5.3).
    pub server_pins: Vec<Vec<u8>>,
    pub account_id: String,
    pub method: InviteMethod,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FriendView {
    pub account_id: String,
    pub active: bool,
    pub verified: bool,
    pub nick: Option<String>,
    pub avatar: Option<Vec<u8>>,
    pub precision: Precision,
    /// Последняя принятая позиция друга; у `Hidden` координат нет.
    pub location: Option<FriendLocation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LocationKind {
    Exact,
    Approx,
    Hidden,
    Frozen,
}

/// Позиция друга для показа на карте. Координаты — градусы × 10⁷.
#[derive(Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct FriendLocation {
    pub kind: LocationKind,
    pub lat_e7: i32,
    pub lon_e7: i32,
    pub accuracy_m: u16,
    /// Время замера (только для показа, §7.5).
    pub timestamp: i64,
}

impl std::fmt::Debug for FriendLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FriendLocation")
            .field("kind", &self.kind)
            .field("coordinates", &"<redacted>")
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

impl From<LocationPayload> for FriendLocation {
    fn from(p: LocationPayload) -> Self {
        let kind = match p.kind {
            ProtoLocationKind::Exact => LocationKind::Exact,
            ProtoLocationKind::Approx => LocationKind::Approx,
            ProtoLocationKind::Hidden => LocationKind::Hidden,
            ProtoLocationKind::Frozen => LocationKind::Frozen,
        };
        Self {
            kind,
            lat_e7: p.lat_e7,
            lon_e7: p.lon_e7,
            accuracy_m: p.accuracy_m,
            timestamp: p.timestamp,
        }
    }
}

/// Событие для UI.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CoreEvent {
    FriendAdded {
        friend: String,
    },
    FriendRemoved {
        friend: String,
    },
    ProfileUpdated {
        friend: String,
    },
    LocationUpdated {
        friend: String,
        location: FriendLocation,
    },
    /// Конверт отброшен; причина — для отладки, без секретов.
    Dropped {
        reason: String,
    },
}

impl From<Event> for CoreEvent {
    fn from(e: Event) -> Self {
        match e {
            Event::FriendAdded { friend } => Self::FriendAdded {
                friend: friend.to_b64(),
            },
            Event::FriendRemoved { friend } => Self::FriendRemoved {
                friend: friend.to_b64(),
            },
            Event::ProfileUpdated { friend } => Self::ProfileUpdated {
                friend: friend.to_b64(),
            },
            Event::LocationUpdated { friend, payload } => Self::LocationUpdated {
                friend: friend.to_b64(),
                location: payload.into(),
            },
            Event::Dropped { reason } => Self::Dropped {
                reason: reason.to_owned(),
            },
        }
    }
}

/// Результат обработки входящих: события и тело `POST /v1/mailbox/ack`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Processed {
    pub events: Vec<CoreEvent>,
    /// JSON `AckRequest`; `None`, если подтверждать нечего.
    pub ack_json: Option<String>,
}

/// Одна попытка отправки по правилам §8.2.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SendBatch {
    /// Сначала `DELETE /v1/slots/{id}` для каждого; если хоть одно не удалось —
    /// конверты в этой попытке не отправлять.
    pub delete_slots: Vec<String>,
    /// JSON `SendRequest` для `POST /v1/envelopes`; `None`, если отправлять нечего.
    pub request_json: Option<String>,
    /// Номера конвертов в том же порядке, что в запросе — для `complete_send`.
    pub ids: Vec<u64>,
}

#[uniffi::export]
impl StayaCore {
    /// Открывает базу (создаёт её и аккаунт при первом запуске).
    /// `db_key` — 32 байта из Keychain / Android Keystore. Базу может держать
    /// открытой только один процесс.
    #[uniffi::constructor]
    pub fn open(db_path: String, db_key: Vec<u8>) -> Result<Arc<Self>, CoreError> {
        let store = Store::open(Path::new(&db_path), &DbKey::from_slice(&db_key)?)?;
        let account = match LocalAccount::load(&store)? {
            Some(a) => a,
            None => LocalAccount::create(&store)?,
        };
        let friends = Friends::load(&store)?;
        let server = ServerBinding::load(&store)?;
        Ok(Arc::new(Self {
            inner: Mutex::new(Inner {
                store,
                account,
                friends,
                server,
            }),
        }))
    }

    pub fn identity(&self) -> Result<IdentityInfo, CoreError> {
        let id = self.lock()?.account.identity();
        Ok(IdentityInfo {
            account_id: id.account_id.to_b64(),
            ik: id.ik.to_vec(),
            sk: id.sk.to_vec(),
        })
    }

    // --- Сервер: регистрация, вход, ключи ---------------------------------

    /// JSON `RegisterRequest` для `POST /v1/accounts`.
    pub fn register_request(&self, invite_code: Option<String>) -> Result<String, CoreError> {
        to_json(&self.lock()?.account.register_request(invite_code))
    }

    /// Подпись challenge для `POST /v1/auth/verify`.
    pub fn sign_auth(&self, domain: String, nonce: Vec<u8>) -> Result<Vec<u8>, CoreError> {
        let nonce: [u8; 32] = nonce.try_into().map_err(|_| CoreError::Invalid("nonce"))?;
        Ok(self.lock()?.account.sign_auth(&domain, &nonce)?.0)
    }

    /// JSON `PublishKeysRequest` для `PUT /v1/keys` или `None`. После успеха — `mark_keys_published`.
    pub fn keys_to_publish(
        &self,
        server_otk_count: u32,
        now: i64,
    ) -> Result<Option<String>, CoreError> {
        let mut g = self.lock()?;
        let Inner { store, account, .. } = &mut *g;
        account
            .keys_to_publish(store, server_otk_count as usize, now)?
            .map(|r| to_json(&r))
            .transpose()
    }

    pub fn mark_keys_published(&self) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, account, .. } = &mut *g;
        account.mark_keys_published(store)
    }

    // --- Друзья -----------------------------------------------------------

    /// Свой профиль; пустой ник — онбординг не завершён.
    pub fn my_profile(&self) -> Result<MyProfile, CoreError> {
        let p = self.lock()?.friends.profile();
        Ok(MyProfile {
            nick: p.nick,
            avatar: p.avatar,
        })
    }

    /// Ник — до 64 байт UTF-8, аватар — до 8 КБ (protocol §6); иначе ошибка.
    pub fn set_profile(&self, nick: String, avatar: Vec<u8>) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.set_profile(store, Profile { nick, avatar })
    }

    /// Ссылка `staya://add?...` для QR или отправки другу — с сервером из
    /// привязки аккаунта (protocol §5.3).
    pub fn create_invite(&self, method: InviteMethod, now: i64) -> Result<String, CoreError> {
        let mut g = self.lock()?;
        let Inner {
            store,
            account,
            friends,
            server,
        } = &mut *g;
        let server = server.as_ref().ok_or(CoreError::NoServer)?.server().clone();
        Ok(friends
            .create_invite(store, &account.identity(), &server, method.into(), now)?
            .to_uri())
    }

    /// Разбирает приглашение: чей ключ запросить через `POST /v1/keys/claim`.
    pub fn parse_invite(&self, uri: String) -> Result<InviteInfo, CoreError> {
        let invite = Invite::parse(&uri)?;
        let method = match invite.method {
            ProtoInviteMethod::Qr => InviteMethod::Qr,
            ProtoInviteMethod::Link => InviteMethod::Link,
        };
        Ok(InviteInfo {
            server: invite.server.host.clone(),
            server_pins: invite.server.pins.iter().map(|p| p.to_vec()).collect(),
            account_id: invite.account_id.to_b64(),
            method,
        })
    }

    /// Принимает приглашение; `claim_response_json` — ответ `POST /v1/keys/claim`.
    pub fn accept_invite(
        &self,
        uri: String,
        claim_response_json: String,
        now: i64,
    ) -> Result<(), CoreError> {
        let invite = Invite::parse(&uri)?;
        let mut g = self.lock()?;
        let Inner {
            store,
            account,
            friends,
            server,
        } = &mut *g;
        // Ключ друга запрашивается у своего сервера: он должен быть сервером приглашения.
        match server {
            Some(b) if b.server().host == invite.server.host => {}
            Some(_) => return Err(CoreError::ServerMismatch),
            None => return Err(CoreError::NoServer),
        }
        let claimed: ClaimResponse = from_json(&claim_response_json)?;
        friends.accept_invite(store, account, &invite, &claimed, now)
    }

    // --- Сервер аккаунта (protocol §5.3) ----------------------------------

    pub fn server(&self) -> Result<Option<ServerInfo>, CoreError> {
        Ok(self.lock()?.server.as_ref().map(ServerInfo::from_binding))
    }

    /// Привязывает аккаунт к серверу (`host` или `host:port` и отпечатки). Другой
    /// сервер при уже существующей привязке — `ServerMismatch`.
    pub fn set_server(&self, host: String, pins: Vec<Vec<u8>>) -> Result<ServerInfo, CoreError> {
        let pins = pins
            .iter()
            .map(|p| {
                <[u8; 32]>::try_from(p.as_slice()).map_err(|_| CoreError::Invalid("server pin"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        bind(self, &ServerRef::new(&host, pins)?)
    }

    /// То же по ссылке на сервер `staya://server?...` или приглашению `staya://add?...`.
    pub fn set_server_from_link(&self, uri: String) -> Result<ServerInfo, CoreError> {
        let server = match ServerRef::parse_link(&uri) {
            Ok(s) => s,
            Err(_) => Invite::parse(&uri)?.server,
        };
        bind(self, &server)
    }

    /// Отвязать аккаунт от сервера — только пока нет ни друзей, ни приглашений в
    /// ожидании: иначе их связь со старым сервером потерялась бы.
    pub fn reset_server(&self) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        if !g.friends.list().is_empty() || g.friends.has_pending_invites() {
            return Err(CoreError::Invalid("account already has friends"));
        }
        ServerBinding::forget(&g.store)?;
        g.server = None;
        Ok(())
    }

    /// Правило доверия к ключу TLS сервера (§5.3). `spki_sha256` — SHA-256 от
    /// SubjectPublicKeyInfo предъявленного ключа; вызывать после обычной проверки
    /// цепочки и до отправки запроса. `Rejected` — разорвать соединение.
    pub fn check_server_key(&self, spki_sha256: Vec<u8>) -> Result<ServerTrust, CoreError> {
        let key: [u8; 32] = spki_sha256
            .try_into()
            .map_err(|_| CoreError::Invalid("server key hash"))?;
        let mut g = self.lock()?;
        let Inner { store, server, .. } = &mut *g;
        server
            .as_mut()
            .ok_or(CoreError::NoServer)?
            .check_key(store, &key)
    }

    /// Действующий токен сессии для `Authorization: Bearer` или `None` — войти.
    pub fn session_token(&self, now: i64) -> Result<Option<Vec<u8>>, CoreError> {
        let g = self.lock()?;
        let b = g.server.as_ref().ok_or(CoreError::NoServer)?;
        Ok(b.session(now).map(<[u8]>::to_vec))
    }

    /// JSON для `POST /v1/auth/challenge`.
    pub fn auth_challenge_request(&self) -> Result<String, CoreError> {
        let g = self.lock()?;
        to_json(&ChallengeRequest {
            account_id: g.account.identity().account_id,
        })
    }

    /// JSON для `POST /v1/auth/verify` по ответу на challenge: подпись с именем
    /// своего сервера (§4.2).
    pub fn auth_verify_request(
        &self,
        challenge_response_json: String,
    ) -> Result<String, CoreError> {
        let challenge: ChallengeResponse = from_json(&challenge_response_json)?;
        let nonce: [u8; 32] = challenge
            .nonce
            .0
            .as_slice()
            .try_into()
            .map_err(|_| CoreError::Invalid("nonce"))?;
        let g = self.lock()?;
        let domain = g.server.as_ref().ok_or(CoreError::NoServer)?.auth_domain();
        let account_id = g.account.identity().account_id;
        to_json(&VerifyRequest {
            account_id,
            nonce: B64(nonce.to_vec()),
            signature: g.account.sign_auth(domain, &nonce)?,
        })
    }

    /// Сохраняет токен из ответа `POST /v1/auth/verify`.
    pub fn complete_login(&self, verify_response_json: String) -> Result<(), CoreError> {
        let resp: VerifyResponse = from_json(&verify_response_json)?;
        if resp.token.0.len() != 32 {
            return Err(CoreError::Invalid("session token"));
        }
        let mut g = self.lock()?;
        let Inner { store, server, .. } = &mut *g;
        server.as_mut().ok_or(CoreError::NoServer)?.set_session(
            store,
            resp.token.0,
            resp.expires_at,
        )
    }

    /// Забыть токен: сервер ответил 401.
    pub fn clear_session(&self) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, server, .. } = &mut *g;
        match server.as_mut() {
            Some(b) => b.clear_session(store),
            None => Ok(()),
        }
    }

    pub fn list_friends(&self) -> Result<Vec<FriendView>, CoreError> {
        let g = self.lock()?;
        Ok(g.friends
            .list()
            .into_iter()
            .map(|f| FriendView {
                account_id: f.account_id.to_b64(),
                active: f.state == FriendState::Active,
                verified: f.verified,
                nick: f.nick,
                avatar: f.avatar,
                precision: g.friends.precision(&f.account_id).unwrap_or_default(),
                location: f.location.map(FriendLocation::from),
            })
            .collect())
    }

    /// 60 цифр группами по 5 (§5.2).
    pub fn safety_code(&self, friend: String) -> Result<String, CoreError> {
        let g = self.lock()?;
        let code = g
            .friends
            .safety_code(&g.account.identity(), &id(&friend)?)?;
        Ok(crate::safety::format_groups(&code))
    }

    pub fn mark_verified(&self, friend: String) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.mark_verified(store, &id(&friend)?)
    }

    pub fn remove_friend(&self, friend: String, notify: bool) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.remove_friend(store, &id(&friend)?, notify)
    }

    // --- Позиция ----------------------------------------------------------

    pub fn set_precision(&self, friend: String, precision: Precision) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.set_precision(store, &id(&friend)?, precision)
    }

    pub fn sharing(&self) -> Result<Sharing, CoreError> {
        Ok(self.lock()?.friends.sharing())
    }

    pub fn set_ghost(&self, ghost: bool) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.set_ghost(store, ghost)
    }

    pub fn set_frozen(&self, frozen: Option<Location>) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.set_frozen(store, frozen)
    }

    /// Заморозка в последней собственной точке; нет точки — `MissingLocation`.
    pub fn freeze_here(&self) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.freeze_here(store)
    }

    /// Кладёт в исходящую очередь пакеты позиции всем друзьям. `None` — из
    /// последнего сохранённого замера (после смены режима или точности).
    pub fn prepare_location_update(
        &self,
        location: Option<Location>,
        now: i64,
    ) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.prepare_location_update(store, location, now)
    }

    // --- Входящие и исходящие ---------------------------------------------

    /// Обрабатывает ответ `GET /v1/mailbox`: сначала очередь, затем слоты (§8.2).
    pub fn process_mailbox(&self, mailbox_json: String, now: i64) -> Result<Processed, CoreError> {
        let mailbox: MailboxResponse = from_json(&mailbox_json)?;
        let mut g = self.lock()?;
        let Inner {
            store,
            account,
            friends,
            ..
        } = &mut *g;
        let mut events = Vec::new();
        let mut acks = Vec::new();
        for item in mailbox.control {
            // Ошибка здесь — только сбой локального хранилища: уже обработанное
            // не подтверждаем, сервер пришлёт снова, повтор безопасен.
            let handled = friends.handle_control(store, account, item.from, &item.data.0, now)?;
            events.extend(handled.events.into_iter().map(CoreEvent::from));
            acks.push(item.seq);
        }
        for slot in mailbox.locations {
            let handled = friends.handle_location(store, slot.from, &slot.data.0)?;
            events.extend(handled.events.into_iter().map(CoreEvent::from));
        }
        let ack_json = if acks.is_empty() {
            None
        } else {
            Some(to_json(&AckRequest { seqs: acks })?)
        };
        Ok(Processed { events, ack_json })
    }

    /// Обрабатывает одно событие WebSocket.
    pub fn process_ws_event(&self, event_json: String, now: i64) -> Result<Processed, CoreError> {
        let event: WsEvent = from_json(&event_json)?;
        let mailbox = match event {
            WsEvent::Control(c) => MailboxResponse {
                control: vec![c],
                locations: vec![],
            },
            WsEvent::Location(l) => MailboxResponse {
                control: vec![],
                locations: vec![l],
            },
        };
        self.process_mailbox(to_json(&mailbox)?, now)
    }

    /// Что отправить сейчас (§8.2).
    pub fn pending_sends(&self) -> Result<SendBatch, CoreError> {
        let pending = self.lock()?.friends.pending_sends();
        let ids: Vec<u64> = pending.envelopes.iter().map(|e| e.id).collect();
        let request_json = if pending.envelopes.is_empty() {
            None
        } else {
            let envelopes = pending
                .envelopes
                .into_iter()
                .map(|e| OutgoingEnvelope {
                    to: e.to,
                    kind: e.kind,
                    data: B64(e.data),
                })
                .collect();
            Some(to_json(&SendRequest { envelopes })?)
        };
        Ok(SendBatch {
            delete_slots: pending.delete_slots.iter().map(AccountId::to_b64).collect(),
            request_json,
            ids,
        })
    }

    /// Ответ сервера на `POST /v1/envelopes`: принятые и окончательно отклонённые
    /// конверты списываются из очереди.
    pub fn complete_send(&self, ids: Vec<u64>, response_json: String) -> Result<(), CoreError> {
        let response: SendResponse = from_json(&response_json)?;
        if response.results.len() != ids.len() {
            return Err(CoreError::Invalid("send response length"));
        }
        let (mut sent, mut rejected) = (Vec::new(), Vec::new());
        for (id, status) in ids.into_iter().zip(response.results) {
            match status {
                EnvelopeStatus::Accepted => sent.push(id),
                EnvelopeStatus::Rejected { .. } => rejected.push(id),
            }
        }
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.complete_send(store, &sent, &rejected)
    }

    /// Сервер подтвердил `DELETE /v1/slots/{friend}`.
    pub fn mark_slot_deleted(&self, friend: String) -> Result<(), CoreError> {
        let mut g = self.lock()?;
        let Inner { store, friends, .. } = &mut *g;
        friends.mark_slot_deleted(store, &id(&friend)?)
    }
}

impl StayaCore {
    fn lock(&self) -> Result<MutexGuard<'_, Inner>, CoreError> {
        // После паники посреди операции состояние в памяти может расходиться
        // с базой: дальше работать нельзя, приложение должно открыть ядро заново.
        self.inner.lock().map_err(|_| CoreError::Poisoned)
    }
}

/// Привязка к серверу; при ошибке прежняя привязка остаётся как была.
fn bind(core: &StayaCore, server: &ServerRef) -> Result<ServerInfo, CoreError> {
    let mut g = core.lock()?;
    let Inner {
        store,
        server: current,
        ..
    } = &mut *g;
    match ServerBinding::bind(store, current.take(), server) {
        Ok(b) => {
            let info = ServerInfo::from_binding(&b);
            *current = Some(b);
            Ok(info)
        }
        Err(e) => {
            *current = ServerBinding::load(store)?;
            Err(e)
        }
    }
}

fn id(b64: &str) -> Result<AccountId, CoreError> {
    Ok(AccountId::from_b64(b64)?)
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value).map_err(|_| CoreError::Invalid("json encode"))
}

fn from_json<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, CoreError> {
    serde_json::from_str(json).map_err(|_| CoreError::Invalid("server response"))
}
