//! Добавление друзей и управляющие сообщения Olm (`docs/protocol.md` §5, §6).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use staya_proto::AccountId;
use staya_proto::api::{ClaimResponse, EnvelopeKind};
use staya_proto::consts::{INVITE_TTL_LINK, INVITE_TTL_QR, OLM_SESSIONS_PER_FRIEND};
use staya_proto::control::{ControlMessage, Profile, SessionKeyBytes};
use staya_proto::envelope::{ControlEnvelope, OlmType};
use staya_proto::invite::{Invite, InviteMethod};
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

/// Конверт, который платформа должна отправить на сервер.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub to: AccountId,
    pub kind: EnvelopeKind,
    pub data: Vec<u8>,
}

/// Что произошло в результате обработки входящего конверта.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Дружба установлена (с обеих сторон или подтверждена другом).
    FriendAdded { friend: AccountId },
    /// Друг обновил ник или аватар.
    ProfileUpdated { friend: AccountId },
    /// Конверт отброшен; причина — для отладки, без секретов.
    Dropped { reason: &'static str },
}

/// Результат обработки: события для UI и конверты для отправки.
#[derive(Debug, Default)]
pub struct Handled {
    pub events: Vec<Event>,
    pub outgoing: Vec<Outgoing>,
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
    /// Текущая и, возможно, следующая входящие Megolm-сессии (§7.5; логика — задача 2.5).
    inbound_megolm: Vec<InboundGroupSession>,
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
        })
    }

    fn save(&self, store: &Store) -> Result<(), CoreError> {
        // JSON не допускает ключи-массивы, поэтому храним список пар.
        let records: Vec<([u8; 16], FriendRecord)> = self
            .friends
            .iter()
            .map(|(id, f)| (*id, f.to_record()))
            .collect();
        save_json(store, FRIENDS_RECORD, &records)?;
        save_json(store, INVITES_RECORD, &self.invites)?;
        save_json(store, PROFILE_RECORD, &self.profile)
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
    pub fn create_invite(
        &mut self,
        store: &Store,
        me: &Identity,
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
    ) -> Result<Outgoing, CoreError> {
        let me = account.identity();
        if invite.account_id == me.account_id {
            return Err(CoreError::InvalidInvite("own invite"));
        }
        if self.friends.contains_key(&invite.account_id.0) {
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
        };
        let out = encrypt_control(&mut record, invite.account_id, &request)?;
        self.friends.insert(invite.account_id.0, record);
        self.save(store)?;
        Ok(out)
    }

    /// Меняет свой профиль и рассылает его активным друзьям.
    pub fn set_profile(
        &mut self,
        store: &Store,
        profile: Profile,
    ) -> Result<Vec<Outgoing>, CoreError> {
        // Проверяем ограничения размеров до сохранения.
        ControlMessage::Profile(profile.clone()).encode()?;
        self.profile = StoredProfile::from(&profile);
        let mut out = Vec::new();
        for (id, f) in self
            .friends
            .iter_mut()
            .filter(|(_, f)| f.state == FriendState::Active)
        {
            out.push(encrypt_control(
                f,
                AccountId(*id),
                &ControlMessage::Profile(profile.clone()),
            )?);
        }
        self.save(store)?;
        Ok(out)
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
        let env = ControlEnvelope::from_bytes(envelope)?;
        let (olm_type, bytes) = env.open()?;
        let message = OlmMessage::from_parts(olm_type as usize, bytes)
            .map_err(|_| CoreError::Crypto("olm decode"))?;

        let mut handled = Handled::default();
        if self.friends.contains_key(&from_hint.0) {
            self.handle_from_friend(account, from_hint, &message, now, &mut handled)?;
        } else {
            self.handle_from_stranger(account, from_hint, &message, now, &mut handled)?;
        }
        // Аккаунт мог израсходовать одноразовый ключ — сохраняем всё вместе.
        account.save(store)?;
        self.save(store)?;
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
                record.inbound_megolm = vec![inbound_megolm(&session_key)?];
                record.profile = Some(StoredProfile::from(&profile));
                record.state = FriendState::Active;
                handled
                    .events
                    .push(Event::FriendAdded { friend: friend_id });
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
                let invite = self.invites.remove(pos);
                record.verified |= invite.method == InviteMethodTag::Qr;
                record.inbound_megolm = vec![inbound_megolm(&session_key)?];
                record.profile = Some(StoredProfile::from(&profile));
                let (outbound, _) = record
                    .outbound_megolm
                    .as_ref()
                    .ok_or(CoreError::Crypto("no megolm session"))?;
                let accept = ControlMessage::FriendAccept {
                    session_key: SessionKeyBytes(outbound.session_key().to_bytes()),
                    profile: Profile::from(&self.profile),
                };
                handled
                    .outgoing
                    .push(encrypt_control(record, friend_id, &accept)?);
                if record.state != FriendState::Active {
                    record.state = FriendState::Active;
                    handled
                        .events
                        .push(Event::FriendAdded { friend: friend_id });
                }
            }
            // SessionShare и Unfriend обрабатываются в задачах 2.5 и 2.6.
            ControlMessage::SessionShare { .. } | ControlMessage::Unfriend => {
                handled.events.push(Event::Dropped {
                    reason: "not implemented yet",
                });
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
            inbound_megolm: vec![inbound_megolm(&session_key)?],
        };
        handled
            .outgoing
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

fn inbound_megolm(key: &SessionKeyBytes) -> Result<InboundGroupSession, CoreError> {
    let key =
        SessionKey::from_bytes(&key.0).map_err(|_| CoreError::Crypto("megolm session key"))?;
    Ok(InboundGroupSession::new(&key, MegolmConfig::version_1()))
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
