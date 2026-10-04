//! Добавление друзей и управляющие сообщения Olm (`docs/protocol.md` §5, §6).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use staya_proto::AccountId;
use staya_proto::api::{ClaimResponse, EnvelopeKind};
use staya_proto::consts::{INVITE_TTL_LINK, INVITE_TTL_QR, OLM_SESSIONS_PER_FRIEND};
use staya_proto::control::{ControlMessage, Profile, SessionKeyBytes};
use staya_proto::envelope::{ControlEnvelope, OlmType};
use staya_proto::invite::{Invite, InviteMethod, ServerRef};
use staya_proto::signing;
use vodozemac::megolm::{
    GroupSession, GroupSessionPickle, InboundGroupSession, InboundGroupSessionPickle,
    SessionConfig as MegolmConfig, SessionKey,
};
use vodozemac::olm::{OlmMessage, Session, SessionConfig as OlmConfig, SessionPickle};
use vodozemac::{Curve25519PublicKey, Ed25519PublicKey, Ed25519Signature};
use zeroize::Zeroizing;

use crate::CoreError;
use crate::account::{Identity, LocalAccount};
use crate::safety;
use crate::store::Store;

const FRIENDS_RECORD: &str = "friends";
const INVITES_RECORD: &str = "invites";
const PROFILE_RECORD: &str = "profile";

mod location;
mod outbox;
pub use location::{Location, Sharing};
pub use outbox::{PendingSends, QueuedEnvelope};

/// Конверт, созданный ядром; попадает в исходящую очередь.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Outgoing {
    pub to: AccountId,
    pub kind: EnvelopeKind,
    pub data: Vec<u8>,
}

/// Что произошло в результате обработки входящего конверта.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Дружба установлена (с обеих сторон или подтверждена другом).
    FriendAdded { friend: AccountId },
    /// Друг удалил нас из друзей.
    FriendRemoved { friend: AccountId },
    /// Друг обновил ник или аватар.
    ProfileUpdated { friend: AccountId },
    /// Новая позиция друга (в т. ч. `Hidden` — друг скрыл позицию от нас).
    LocationUpdated {
        friend: AccountId,
        payload: staya_proto::location::LocationPayload,
    },
    /// Конверт отброшен; причина — для отладки, без секретов.
    Dropped { reason: &'static str },
}

/// Результат обработки: события для UI. Ответы ушли в исходящую очередь.
#[derive(Debug, Default)]
pub struct Handled {
    pub events: Vec<Event>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FriendState {
    /// Мы приняли приглашение и ждём `FriendAccept`.
    AwaitingAccept,
    Active,
}

struct OlmEntry {
    session: Session,
    created_at: i64,
    /// Когда сессия последний раз успешно расшифровала сообщение от друга.
    last_received_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredProfile {
    nick: String,
    avatar: Vec<u8>,
}

impl From<&Profile> for StoredProfile {
    fn from(p: &Profile) -> Self {
        Self {
            nick: p.nick.clone(),
            avatar: p.avatar.clone(),
        }
    }
}

impl From<&StoredProfile> for Profile {
    fn from(p: &StoredProfile) -> Self {
        Self {
            nick: p.nick.clone(),
            avatar: p.avatar.clone(),
        }
    }
}

struct Friend {
    ik: [u8; 32],
    sk: [u8; 32],
    state: FriendState,
    /// Ключи получены при встрече (QR) или код безопасности сверен.
    verified: bool,
    profile: Option<StoredProfile>,
    /// Свежие — в конце.
    olm: Vec<OlmEntry>,
    /// Исходящая Megolm-сессия к другу и время её создания.
    outbound_megolm: Option<(GroupSession, i64)>,
    /// Входящие Megolm-сессии, от старой к новой (§7.5).
    inbound_megolm: Vec<InboundGroupSession>,
    /// Какую точность мы показываем этому другу.
    precision: Precision,
}

/// Точность, с которой другу видна наша позиция (§7.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum Precision {
    #[default]
    Exact,
    Approx,
    Hidden,
}

/// Форма [`Friend`] для хранения: сессии в виде pickle.
#[derive(Serialize, Deserialize)]
struct FriendRecord {
    ik: [u8; 32],
    sk: [u8; 32],
    state: FriendState,
    verified: bool,
    profile: Option<StoredProfile>,
    olm: Vec<(SessionPickle, i64, Option<i64>)>,
    outbound_megolm: Option<(GroupSessionPickle, i64)>,
    inbound_megolm: Vec<InboundGroupSessionPickle>,
    #[serde(default)]
    precision: Precision,
}

impl Friend {
    fn to_record(&self) -> FriendRecord {
        FriendRecord {
            ik: self.ik,
            sk: self.sk,
            state: self.state,
            verified: self.verified,
            profile: self.profile.clone(),
            olm: self
                .olm
                .iter()
                .map(|e| (e.session.pickle(), e.created_at, e.last_received_at))
                .collect(),
            outbound_megolm: self
                .outbound_megolm
                .as_ref()
                .map(|(s, at)| (s.pickle(), *at)),
            inbound_megolm: self
                .inbound_megolm
                .iter()
                .map(InboundGroupSession::pickle)
                .collect(),
            precision: self.precision,
        }
    }

    fn from_record(r: FriendRecord) -> Self {
        Self {
            ik: r.ik,
            sk: r.sk,
            state: r.state,
            verified: r.verified,
            profile: r.profile,
            olm: r
                .olm
                .into_iter()
                .map(|(p, created_at, last_received_at)| OlmEntry {
                    session: Session::from_pickle(p),
                    created_at,
                    last_received_at,
                })
                .collect(),
            outbound_megolm: r
                .outbound_megolm
                .map(|(p, at)| (GroupSession::from_pickle(p), at)),
            inbound_megolm: r
                .inbound_megolm
                .into_iter()
                .map(InboundGroupSession::from_pickle)
                .collect(),
            precision: r.precision,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct PendingInvite {
    token: [u8; 16],
    method: InviteMethodTag,
    expires_at: i64,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
enum InviteMethodTag {
    Qr,
    Link,
}

impl From<InviteMethod> for InviteMethodTag {
    fn from(m: InviteMethod) -> Self {
        match m {
            InviteMethod::Qr => Self::Qr,
            InviteMethod::Link => Self::Link,
        }
    }
}

/// Публичные сведения о друге для UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendInfo {
    pub account_id: AccountId,
    pub state: FriendState,
    pub verified: bool,
    pub nick: Option<String>,
    pub avatar: Option<Vec<u8>>,
}

/// Список друзей и приглашений поверх аккаунта и хранилища.
pub struct Friends {
    friends: BTreeMap<[u8; 16], Friend>,
    invites: Vec<PendingInvite>,
    profile: StoredProfile,
    sharing: location::Sharing,
    /// Пакеты позиции от сессий, которых ещё нет (§7.5): по подсказке отправителя, только последний.
    held: Vec<([u8; 16], Vec<u8>)>,
    outbox: outbox::Outbox,
}

impl Friends {
    pub fn load(store: &Store) -> Result<Self, CoreError> {
        Ok(Self {
            friends: load_json::<Vec<([u8; 16], FriendRecord)>>(store, FRIENDS_RECORD)?
                .unwrap_or_default()
                .into_iter()
                .map(|(id, r)| (id, Friend::from_record(r)))
                .collect(),
            invites: load_json(store, INVITES_RECORD)?.unwrap_or_default(),
            profile: load_json(store, PROFILE_RECORD)?.unwrap_or(StoredProfile {
                nick: String::new(),
                avatar: vec![],
            }),
            sharing: load_json(store, location::SHARING_RECORD)?.unwrap_or_default(),
            held: load_json(store, location::HELD_RECORD)?.unwrap_or_default(),
            outbox: load_json(store, outbox::OUTBOX_RECORD)?.unwrap_or_default(),
        })
    }

    /// Сохраняет друзей, приглашения и профиль одной транзакцией.
    fn save(&self, store: &Store) -> Result<(), CoreError> {
        store.atomically(|| self.write_records(store))
    }

    /// Запись без собственной транзакции — для вызова внутри `atomically`.
    fn write_records(&self, store: &Store) -> Result<(), CoreError> {
        // JSON не допускает ключи-массивы, поэтому храним список пар.
        let records: Vec<([u8; 16], FriendRecord)> = self
            .friends
            .iter()
            .map(|(id, f)| (*id, f.to_record()))
            .collect();
        save_json(store, FRIENDS_RECORD, &records)?;
        save_json(store, INVITES_RECORD, &self.invites)?;
        save_json(store, PROFILE_RECORD, &self.profile)?;
        save_json(store, location::SHARING_RECORD, &self.sharing)?;
        save_json(store, location::HELD_RECORD, &self.held)?;
        save_json(store, outbox::OUTBOX_RECORD, &self.outbox)
    }

    pub fn list(&self) -> Vec<FriendInfo> {
        self.friends
            .iter()
            .map(|(id, f)| FriendInfo {
                account_id: AccountId(*id),
                state: f.state,
                verified: f.verified,
                nick: f.profile.as_ref().map(|p| p.nick.clone()),
                avatar: f.profile.as_ref().map(|p| p.avatar.clone()),
            })
            .collect()
    }

    /// Код безопасности с другом (§5.2).
    pub fn safety_code(&self, me: &Identity, friend: &AccountId) -> Result<String, CoreError> {
        let f = self
            .friends
            .get(&friend.0)
            .ok_or(CoreError::UnknownFriend)?;
        let them = Identity {
            account_id: *friend,
            ik: f.ik,
            sk: f.sk,
        };
        Ok(safety::safety_code(me, &them))
    }

    /// Пользователь сверил код безопасности с другом.
    pub fn mark_verified(&mut self, store: &Store, friend: &AccountId) -> Result<(), CoreError> {
        self.friends
            .get_mut(&friend.0)
            .ok_or(CoreError::UnknownFriend)?
            .verified = true;
        self.save(store)
    }

    /// Создаёт приглашение (A-сторона, §5 шаги 1–2).
    /// `server` — сервер, на котором живёт этот аккаунт (protocol §5.3): друг
    /// подключится к нему же.
    pub fn create_invite(
        &mut self,
        store: &Store,
        me: &Identity,
        server: &ServerRef,
        method: InviteMethod,
        now: i64,
    ) -> Result<Invite, CoreError> {
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|_| CoreError::Random)?;
        let ttl = match method {
            InviteMethod::Qr => INVITE_TTL_QR,
            InviteMethod::Link => INVITE_TTL_LINK,
        };
        self.prune_invites(now);
        self.invites.push(PendingInvite {
            token,
            method: method.into(),
            expires_at: now + ttl.as_secs() as i64,
        });
        self.save(store)?;
        Ok(Invite {
            server: server.clone(),
            account_id: me.account_id,
            ik: me.ik,
            sk: me.sk,
            token,
            method,
        })
    }

    fn prune_invites(&mut self, now: i64) {
        self.invites.retain(|i| i.expires_at > now);
    }

    /// Принимает приглашение (B-сторона, §5 шаги 3–4).
    ///
    /// `claimed` — ответ сервера на `claim` одноразового ключа пригласившего.
    pub fn accept_invite(
        &mut self,
        store: &Store,
        account: &LocalAccount,
        invite: &Invite,
        claimed: &ClaimResponse,
        now: i64,
    ) -> Result<(), CoreError> {
        let me = account.identity();
        if invite.account_id == me.account_id {
            return Err(CoreError::InvalidInvite("own invite"));
        }
        // Можно заново принять приглашение, пока прошлая попытка не подтверждена
        // (например, токен истёк): старые сессии заменяются новыми.
        if self
            .friends
            .get(&invite.account_id.0)
            .is_some_and(|f| f.state == FriendState::Active)
        {
            return Err(CoreError::InvalidInvite("already a friend"));
        }

        // Ключ, выданный сервером, должен быть подписан ключом из приглашения (§4.3).
        let otk: [u8; 32] = claimed
            .key
            .key
            .0
            .as_slice()
            .try_into()
            .map_err(|_| CoreError::InvalidInvite("key"))?;
        let signed = if claimed.is_fallback {
            signing::fallback_key(&otk)
        } else {
            signing::one_time_key(&otk)
        };
        verify(&invite.sk, &signed, &claimed.key.signature.0)
            .map_err(|_| CoreError::InvalidInvite("key signature"))?;

        let olm = account
            .olm()
            .create_outbound_session(
                OlmConfig::version_1(),
                Curve25519PublicKey::from_bytes(invite.ik),
                Curve25519PublicKey::from_bytes(otk),
            )
            .map_err(|_| CoreError::Crypto("olm outbound session"))?;

        let megolm = GroupSession::new(MegolmConfig::version_1());
        let request = ControlMessage::FriendRequest {
            token: invite.token,
            account_id: me.account_id,
            sk: me.sk,
            session_key: SessionKeyBytes(megolm.session_key().to_bytes()),
            profile: Profile::from(&self.profile),
        };

        let mut record = Friend {
            ik: invite.ik,
            sk: invite.sk,
            state: FriendState::AwaitingAccept,
            verified: invite.method == InviteMethod::Qr,
            profile: None,
            olm: vec![OlmEntry {
                session: olm,
                created_at: now,
                last_received_at: None,
            }],
            outbound_megolm: Some((megolm, now)),
            inbound_megolm: vec![],
            precision: Precision::default(),
        };
        let out = encrypt_control(&mut record, invite.account_id, &request)?;
        // Прошлая неподтверждённая попытка больше не нужна.
        self.outbox.drop_for(&invite.account_id);
        self.outbox.push(out);
        self.friends.insert(invite.account_id.0, record);
        self.save(store)
    }

    /// Меняет свой профиль и рассылает его активным друзьям.
    /// Есть выданные, ещё не использованные приглашения.
    pub fn has_pending_invites(&self) -> bool {
        !self.invites.is_empty()
    }

    /// Свой профиль (ник и аватар).
    pub fn profile(&self) -> Profile {
        Profile::from(&self.profile)
    }

    pub fn set_profile(&mut self, store: &Store, profile: Profile) -> Result<(), CoreError> {
        // Проверяем ограничения размеров до сохранения.
        ControlMessage::Profile(profile.clone()).encode()?;
        self.profile = StoredProfile::from(&profile);
        for (id, f) in self
            .friends
            .iter_mut()
            .filter(|(_, f)| f.state == FriendState::Active)
        {
            self.outbox.push(encrypt_control(
                f,
                AccountId(*id),
                &ControlMessage::Profile(profile.clone()),
            )?);
        }
        self.save(store)
    }

    /// Что нужно отправить на сервер: сначала удаления слотов, затем управляющие
    /// по порядку, затем позиции (§8.2).
    pub fn pending_sends(&self) -> PendingSends {
        self.outbox.pending()
    }

    /// Сервер принял конверты с этими номерами.
    pub fn mark_sent(&mut self, store: &Store, ids: &[u64]) -> Result<(), CoreError> {
        self.outbox.mark_sent(ids);
        self.save(store)
    }

    /// Сервер окончательно отклонил конверты (§8.2): списываем, чтобы они не
    /// повторялись вечно и не задерживали остальные.
    pub fn mark_rejected(&mut self, store: &Store, ids: &[u64]) -> Result<(), CoreError> {
        self.outbox.mark_sent(ids);
        self.save(store)
    }

    /// Итог одной отправки (§8.2): принятые и окончательно отклонённые списываются
    /// одной записью.
    pub fn complete_send(
        &mut self,
        store: &Store,
        sent: &[u64],
        rejected: &[u64],
    ) -> Result<(), CoreError> {
        self.outbox.mark_sent(sent);
        self.outbox.mark_sent(rejected);
        self.save(store)
    }

    /// Сервер подтвердил удаление нашего слота у получателя.
    pub fn mark_slot_deleted(
        &mut self,
        store: &Store,
        recipient: &AccountId,
    ) -> Result<(), CoreError> {
        self.outbox.mark_slot_deleted(recipient);
        self.save(store)
    }

    /// Удаляет друга (§9): сессии уничтожаются, отправка прекращается, наш слот
    /// у него удаляется на сервере. С `notify` друг получит `Unfriend`.
    pub fn remove_friend(
        &mut self,
        store: &Store,
        friend: &AccountId,
        notify: bool,
    ) -> Result<(), CoreError> {
        let mut record = self
            .friends
            .remove(&friend.0)
            .ok_or(CoreError::UnknownFriend)?;
        self.forget(friend);
        if notify && record.state == FriendState::Active {
            let out = encrypt_control(&mut record, *friend, &ControlMessage::Unfriend)?;
            self.outbox.push(out);
        }
        // `record` со всеми ключами выходит из области видимости здесь.
        self.save(store)
    }

    /// Всё, что осталось от друга после удаления записи: очередь, отложенные пакеты, слот.
    fn forget(&mut self, friend: &AccountId) {
        self.outbox.drop_for(friend);
        self.outbox.request_slot_deletion(friend);
        self.held.retain(|(id, _)| *id != friend.0);
    }

    /// Обрабатывает управляющий конверт из очереди.
    ///
    /// `from_hint` — отправитель по словам сервера; ему не доверяем (§7.5):
    /// для известного друга отправителя определяет расшифровавшая сессия.
    pub fn handle_control(
        &mut self,
        store: &Store,
        account: &mut LocalAccount,
        from_hint: AccountId,
        envelope: &[u8],
        now: i64,
    ) -> Result<Handled, CoreError> {
        let mut handled = Handled::default();
        // Всё, что контролируют сервер или собеседник, не должно давать `Err`:
        // иначе сообщение не подтвердится и навсегда застрянет в очереди (§8.2).
        let Some(message) = parse_control(envelope) else {
            handled.events.push(Event::Dropped {
                reason: "malformed envelope",
            });
            return Ok(handled);
        };
        if self.friends.contains_key(&from_hint.0) {
            self.handle_from_friend(account, from_hint, &message, now, &mut handled)?;
        } else {
            self.handle_from_stranger(account, from_hint, &message, now, &mut handled)?;
        }
        // Аккаунт мог израсходовать одноразовый ключ — сохраняем всё одной транзакцией.
        store.atomically(|| {
            account.save(store)?;
            self.write_records(store)
        })?;
        Ok(handled)
    }

    fn handle_from_friend(
        &mut self,
        account: &mut LocalAccount,
        friend_id: AccountId,
        message: &OlmMessage,
        now: i64,
        handled: &mut Handled,
    ) -> Result<(), CoreError> {
        let record = self
            .friends
            .get_mut(&friend_id.0)
            .expect("checked by caller");
        let Some(plaintext) = decrypt_from_friend(record, account, message, now)? else {
            handled.events.push(Event::Dropped {
                reason: "no olm session decrypts",
            });
            return Ok(());
        };
        let msg = match ControlMessage::decode(&plaintext) {
            Ok(m) => m,
            Err(_) => {
                handled.events.push(Event::Dropped {
                    reason: "malformed control message",
                });
                return Ok(());
            }
        };
        match msg {
            ControlMessage::FriendAccept {
                session_key,
                profile,
            } => {
                if record.state != FriendState::AwaitingAccept {
                    // Ответ на встречное приглашение, дружба уже установлена.
                    handled.events.push(Event::Dropped {
                        reason: "unexpected accept",
                    });
                    return Ok(());
                }
                let Some(inbound) = inbound_megolm(&session_key) else {
                    handled.events.push(Event::Dropped {
                        reason: "malformed session key",
                    });
                    return Ok(());
                };
                record.inbound_megolm = vec![inbound];
                record.profile = Some(StoredProfile::from(&profile));
                record.state = FriendState::Active;
                handled
                    .events
                    .push(Event::FriendAdded { friend: friend_id });
                // Пакет позиции мог прийти по WebSocket раньше подтверждения.
                self.retry_held(handled);
            }
            ControlMessage::Profile(profile) => {
                record.profile = Some(StoredProfile::from(&profile));
                handled
                    .events
                    .push(Event::ProfileUpdated { friend: friend_id });
            }
            // Встречное приглашение: оба приняли QR друг друга одновременно (§6.3).
            // Действующий токен завершает дружбу; иначе это повтор — игнорируем.
            ControlMessage::FriendRequest {
                token,
                account_id,
                sk,
                session_key,
                profile,
            } => {
                self.invites.retain(|i| i.expires_at > now);
                let Some(pos) = self.invites.iter().position(|i| i.token == token) else {
                    handled.events.push(Event::Dropped {
                        reason: "duplicate friend request",
                    });
                    return Ok(());
                };
                let record = self
                    .friends
                    .get_mut(&friend_id.0)
                    .expect("checked by caller");
                if account_id != friend_id || sk != record.sk {
                    handled.events.push(Event::Dropped {
                        reason: "friend request does not match friend",
                    });
                    return Ok(());
                }
                let Some(inbound) = inbound_megolm(&session_key) else {
                    handled.events.push(Event::Dropped {
                        reason: "malformed session key",
                    });
                    return Ok(());
                };
                let invite = self.invites.remove(pos);
                record.verified |= invite.method == InviteMethodTag::Qr;
                // После установления дружбы список сессий меняет только SessionShare,
                // иначе нарушился бы порядок, на котором держится защита от отката (§7.5).
                if record.state != FriendState::Active {
                    record.inbound_megolm = vec![inbound];
                }
                record.profile = Some(StoredProfile::from(&profile));
                let (outbound, _) = record
                    .outbound_megolm
                    .as_ref()
                    .ok_or(CoreError::Crypto("no megolm session"))?;
                let accept = ControlMessage::FriendAccept {
                    session_key: SessionKeyBytes(outbound.session_key().to_bytes()),
                    profile: Profile::from(&self.profile),
                };
                self.outbox
                    .push(encrypt_control(record, friend_id, &accept)?);
                if record.state != FriendState::Active {
                    record.state = FriendState::Active;
                    handled
                        .events
                        .push(Event::FriendAdded { friend: friend_id });
                }
                self.retry_held(handled);
            }
            ControlMessage::SessionShare { session_key } => {
                if !Self::add_inbound_session(record, &session_key) {
                    handled.events.push(Event::Dropped {
                        reason: "malformed session key",
                    });
                }
                // Пакеты, ждавшие этого ключа, теперь можно расшифровать.
                self.retry_held(handled);
            }
            // Unfriend обрабатывается в задаче 2.6.
            // Друг удалил нас: зеркально удаляем его и свой слот у него (§9).
            ControlMessage::Unfriend => {
                self.friends.remove(&friend_id.0);
                self.forget(&friend_id);
                handled
                    .events
                    .push(Event::FriendRemoved { friend: friend_id });
            }
        }
        Ok(())
    }

    fn handle_from_stranger(
        &mut self,
        account: &mut LocalAccount,
        from_hint: AccountId,
        message: &OlmMessage,
        now: i64,
        handled: &mut Handled,
    ) -> Result<(), CoreError> {
        // От незнакомца принимаем только PreKey-сообщение с FriendRequest.
        let OlmMessage::PreKey(prekey) = message else {
            handled.events.push(Event::Dropped {
                reason: "normal message from stranger",
            });
            return Ok(());
        };
        let their_ik = prekey.identity_key();
        let Ok(created) =
            account
                .olm_mut()
                .create_inbound_session(OlmConfig::version_1(), their_ik, prekey)
        else {
            handled.events.push(Event::Dropped {
                reason: "cannot create inbound session",
            });
            return Ok(());
        };
        let plaintext = Zeroizing::new(created.plaintext);
        let Ok(ControlMessage::FriendRequest {
            token,
            account_id,
            sk,
            session_key,
            profile,
        }) = ControlMessage::decode(&plaintext)
        else {
            handled.events.push(Event::Dropped {
                reason: "stranger did not send a friend request",
            });
            return Ok(());
        };
        if account_id != from_hint {
            handled.events.push(Event::Dropped {
                reason: "sender id mismatch",
            });
            return Ok(());
        }
        let Some(inbound) = inbound_megolm(&session_key) else {
            handled.events.push(Event::Dropped {
                reason: "malformed session key",
            });
            return Ok(());
        };
        self.prune_invites(now);
        let Some(pos) = self.invites.iter().position(|i| i.token == token) else {
            handled.events.push(Event::Dropped {
                reason: "unknown or expired invite token",
            });
            return Ok(());
        };
        let invite = self.invites.remove(pos);

        let outbound = GroupSession::new(MegolmConfig::version_1());
        let accept = ControlMessage::FriendAccept {
            session_key: SessionKeyBytes(outbound.session_key().to_bytes()),
            profile: Profile::from(&self.profile),
        };
        let mut record = Friend {
            ik: their_ik.to_bytes(),
            sk,
            state: FriendState::Active,
            verified: invite.method == InviteMethodTag::Qr,
            profile: Some(StoredProfile::from(&profile)),
            olm: vec![OlmEntry {
                session: created.session,
                created_at: now,
                last_received_at: Some(now),
            }],
            outbound_megolm: Some((outbound, now)),
            inbound_megolm: vec![inbound],
            precision: Precision::default(),
        };
        self.outbox
            .push(encrypt_control(&mut record, account_id, &accept)?);
        self.friends.insert(account_id.0, record);
        handled
            .events
            .push(Event::FriendAdded { friend: account_id });
        Ok(())
    }
}

/// Пробует Olm-сессии друга; при новом PreKey-сообщении создаёт входящую сессию (§6.3).
fn decrypt_from_friend(
    record: &mut Friend,
    account: &mut LocalAccount,
    message: &OlmMessage,
    now: i64,
) -> Result<Option<Zeroizing<Vec<u8>>>, CoreError> {
    let prekey_session_id = match message {
        OlmMessage::PreKey(p) => Some(p.session_id()),
        OlmMessage::Normal(_) => None,
    };
    for entry in record.olm.iter_mut().rev() {
        if let Some(id) = &prekey_session_id
            && &entry.session.session_id() != id
        {
            continue;
        }
        // Неудачная расшифровка не меняет состояние сессии.
        if let Ok(pt) = entry.session.decrypt(message) {
            entry.last_received_at = Some(now);
            return Ok(Some(Zeroizing::new(pt)));
        }
    }
    let OlmMessage::PreKey(prekey) = message else {
        return Ok(None);
    };
    let their_ik = Curve25519PublicKey::from_bytes(record.ik);
    if prekey.identity_key() != their_ik {
        return Ok(None);
    }
    let Ok(created) =
        account
            .olm_mut()
            .create_inbound_session(OlmConfig::version_1(), their_ik, prekey)
    else {
        return Ok(None);
    };
    record.olm.push(OlmEntry {
        session: created.session,
        created_at: now,
        last_received_at: Some(now),
    });
    if record.olm.len() > OLM_SESSIONS_PER_FRIEND {
        record.olm.remove(0);
    }
    Ok(Some(Zeroizing::new(created.plaintext)))
}

/// Шифрует управляющее сообщение сессией, которая последней что-то расшифровала,
/// а если таких нет — самой свежей (§6.3).
fn encrypt_control(
    record: &mut Friend,
    to: AccountId,
    msg: &ControlMessage,
) -> Result<Outgoing, CoreError> {
    let plaintext = Zeroizing::new(msg.encode()?);
    let idx = record
        .olm
        .iter()
        .enumerate()
        .max_by_key(|(i, e)| (e.last_received_at, *i))
        .map(|(i, _)| i)
        .ok_or(CoreError::Crypto("no olm session"))?;
    let message = record.olm[idx]
        .session
        .encrypt(plaintext.as_slice())
        .map_err(|_| CoreError::Crypto("olm encrypt"))?;
    let (t, bytes) = message.to_parts();
    let olm_type = if t == 0 {
        OlmType::PreKey
    } else {
        OlmType::Normal
    };
    let env = ControlEnvelope::seal(olm_type, &bytes)?;
    Ok(Outgoing {
        to,
        kind: EnvelopeKind::Control,
        data: env.as_bytes().to_vec(),
    })
}

/// Входящая Megolm-сессия из ключа, присланного другом; `None` — ключ испорчен.
fn inbound_megolm(key: &SessionKeyBytes) -> Option<InboundGroupSession> {
    let key = SessionKey::from_bytes(&key.0).ok()?;
    Some(InboundGroupSession::new(&key, MegolmConfig::version_1()))
}

fn parse_control(envelope: &[u8]) -> Option<OlmMessage> {
    let env = ControlEnvelope::from_bytes(envelope).ok()?;
    let (olm_type, bytes) = env.open().ok()?;
    OlmMessage::from_parts(olm_type as usize, bytes).ok()
}

fn verify(sk: &[u8; 32], message: &[u8], signature: &[u8]) -> Result<(), ()> {
    let pk = Ed25519PublicKey::from_slice(sk).map_err(|_| ())?;
    let sig = Ed25519Signature::from_slice(signature).map_err(|_| ())?;
    pk.verify(message, &sig).map_err(|_| ())
}

fn load_json<T: serde::de::DeserializeOwned>(
    store: &Store,
    name: &str,
) -> Result<Option<T>, CoreError> {
    let Some(bytes) = store.get_secret(name)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| CoreError::Corrupted)
}

fn save_json<T: Serialize>(store: &Store, name: &str, value: &T) -> Result<(), CoreError> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| CoreError::Corrupted)?);
    store.put_secret(name, &bytes)
}
