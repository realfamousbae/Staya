//! Канал позиций на Megolm (`docs/protocol.md` §7).

use std::fmt;

use serde::{Deserialize, Serialize};
use staya_proto::AccountId;
use staya_proto::api::EnvelopeKind;
use staya_proto::consts::{MEGOLM_ROTATION_AGE, MEGOLM_ROTATION_MESSAGES};
use staya_proto::control::{ControlMessage, SessionKeyBytes};
use staya_proto::envelope::LocationEnvelope;
use staya_proto::location::{LocationKind, LocationPayload, snap_to_grid};
use vodozemac::megolm::{
    DecryptionError, GroupSession, MegolmMessage, SessionConfig as MegolmConfig,
};

use super::{Event, Friend, FriendState, Friends, Handled, Outgoing, Precision, encrypt_control};
use crate::CoreError;
use crate::store::Store;

pub(super) const SHARING_RECORD: &str = "sharing";
pub(super) const HELD_RECORD: &str = "held_locations";

/// Сколько входящих Megolm-сессий держим на друга: текущая и присланные вперёд.
const INBOUND_SESSIONS_PER_FRIEND: usize = 3;

/// Замер позиции с устройства. Координаты — градусы × 10⁷.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub lat_e7: i32,
    pub lon_e7: i32,
    pub accuracy_m: u16,
    /// Unix-время замера, секунды. Только для показа другу.
    pub timestamp: i64,
}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Location")
            .field("coordinates", &"<redacted>")
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

/// Глобальные режимы: призрак и заморозка (§7.3–7.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sharing {
    pub ghost: bool,
    /// Зафиксированная позиция; `None` — заморозка выключена.
    pub frozen: Option<Location>,
}

impl Friends {
    pub fn sharing(&self) -> Sharing {
        self.sharing
    }

    pub fn set_ghost(&mut self, store: &Store, ghost: bool) -> Result<(), CoreError> {
        self.sharing.ghost = ghost;
        self.save(store)
    }

    /// Замораживает позицию (`Some`) или снимает заморозку (`None`).
    pub fn set_frozen(&mut self, store: &Store, frozen: Option<Location>) -> Result<(), CoreError> {
        self.sharing.frozen = frozen;
        self.save(store)
    }

    pub fn set_precision(
        &mut self,
        store: &Store,
        friend: &AccountId,
        precision: Precision,
    ) -> Result<(), CoreError> {
        self.friends
            .get_mut(&friend.0)
            .ok_or(CoreError::UnknownFriend)?
            .precision = precision;
        self.save(store)
    }

    pub fn precision(&self, friend: &AccountId) -> Option<Precision> {
        self.friends.get(&friend.0).map(|f| f.precision)
    }

    /// Готовит одну отправку: по пакету позиции каждому активному другу (§7.4).
    ///
    /// `location` нужна, только если не включены призрак или заморозка.
    /// `now` — время отправки: им помечаются пакеты `Hidden`.
    /// Если пора ротировать сессию, `SessionShare` идёт в той же отправке раньше пакета (§7.5).
    pub fn prepare_location_update(
        &mut self,
        store: &Store,
        location: Option<Location>,
        now: i64,
    ) -> Result<Vec<Outgoing>, CoreError> {
        let sharing = self.sharing;
        if !sharing.ghost && sharing.frozen.is_none() && location.is_none() {
            return Err(CoreError::MissingLocation);
        }
        let mut out = Vec::new();
        for (id, friend) in self
            .friends
            .iter_mut()
            .filter(|(_, f)| f.state == FriendState::Active)
        {
            let to = AccountId(*id);
            let payload = payload_for(friend.precision, sharing, location, now);
            if let Some(share) = rotate_if_due(friend, to, now)? {
                out.push(share);
            }
            let (session, _) = friend
                .outbound_megolm
                .as_mut()
                .ok_or(CoreError::Crypto("no megolm session"))?;
            let message = session.encrypt(payload.encode()?);
            let envelope = LocationEnvelope::seal(&message.to_bytes())?;
            out.push(Outgoing {
                to,
                kind: EnvelopeKind::Location,
                data: envelope.0.to_vec(),
            });
        }
        self.save(store)?;
        Ok(out)
    }

    /// Обрабатывает последний пакет позиции от отправителя.
    ///
    /// `from_hint` — подсказка сервера, чьи сессии пробовать первыми; пакет
    /// приписывается тому другу, чья сессия его расшифровала (§7.5).
    pub fn handle_location(
        &mut self,
        store: &Store,
        from_hint: AccountId,
        envelope: &[u8],
    ) -> Result<Handled, CoreError> {
        let mut handled = Handled::default();
        let event = self.decrypt_location(from_hint, envelope);
        if matches!(
            event,
            Event::Dropped {
                reason: REASON_UNKNOWN_SESSION
            }
        ) && self.friends.contains_key(&from_hint.0)
        {
            // Ключ сессии может прийти следующим управляющим сообщением: держим пакет.
            // Только для известных друзей и только последний — сервер не раздует список.
            self.held.retain(|(id, _)| *id != from_hint.0);
            self.held.push((from_hint.0, envelope.to_vec()));
        }
        handled.events.push(event);
        self.save(store)?;
        Ok(handled)
    }

    /// Сколько пакетов ждут своего ключа (для диагностики и тестов).
    pub fn held_packet_count(&self) -> usize {
        self.held.len()
    }

    /// Повторяет отложенные пакеты после того, как пришли новые ключи.
    pub(super) fn retry_held(&mut self, handled: &mut Handled) {
        let held = std::mem::take(&mut self.held);
        for (hint, envelope) in held {
            match self.decrypt_location(AccountId(hint), &envelope) {
                Event::Dropped {
                    reason: REASON_UNKNOWN_SESSION,
                } => self.held.push((hint, envelope)),
                event @ Event::LocationUpdated { .. } => handled.events.push(event),
                Event::Dropped { .. }
                | Event::FriendAdded { .. }
                | Event::ProfileUpdated { .. } => {}
            }
        }
    }

    fn decrypt_location(&mut self, from_hint: AccountId, envelope: &[u8]) -> Event {
        let Some(message) = parse_location(envelope) else {
            return Event::Dropped {
                reason: "malformed location envelope",
            };
        };
        // Сначала сессии друга из подсказки, потом остальных.
        let mut order: Vec<[u8; 16]> = Vec::with_capacity(self.friends.len());
        if self.friends.contains_key(&from_hint.0) {
            order.push(from_hint.0);
        }
        order.extend(self.friends.keys().filter(|id| **id != from_hint.0));

        for id in order {
            let friend = self.friends.get_mut(&id).expect("id from the map");
            if friend.state != FriendState::Active {
                continue;
            }
            match try_inbound(friend, &message) {
                Attempt::Decrypted { pos, plaintext } => {
                    return accept_payload(friend, AccountId(id), pos, &plaintext);
                }
                // Подпись сессии друга верна, но индекс уже пройден: пакет уже был.
                Attempt::AlreadySeen => {
                    return Event::Dropped {
                        reason: REASON_DUPLICATE,
                    };
                }
                Attempt::NotThisFriend => {}
            }
        }
        Event::Dropped {
            reason: REASON_UNKNOWN_SESSION,
        }
    }

    /// Новый ключ Megolm от друга (`SessionShare`).
    pub(super) fn add_inbound_session(friend: &mut Friend, session_key: &SessionKeyBytes) -> bool {
        let Some(session) = super::inbound_megolm(session_key) else {
            return false;
        };
        if friend
            .inbound_megolm
            .iter()
            .any(|s| s.session_id() == session.session_id())
        {
            return true;
        }
        friend.inbound_megolm.push(session);
        if friend.inbound_megolm.len() > INBOUND_SESSIONS_PER_FRIEND {
            friend.inbound_megolm.remove(0);
        }
        true
    }
}

const REASON_UNKNOWN_SESSION: &str = "no megolm session decrypts";
const REASON_DUPLICATE: &str = "location already seen";

enum Attempt {
    Decrypted { pos: usize, plaintext: Vec<u8> },
    AlreadySeen,
    NotThisFriend,
}

fn parse_location(envelope: &[u8]) -> Option<MegolmMessage> {
    let env = LocationEnvelope::from_bytes(envelope).ok()?;
    MegolmMessage::from_bytes(env.open().ok()?).ok()
}

/// Пробует входящие сессии друга, от новой к старой.
fn try_inbound(friend: &mut Friend, message: &MegolmMessage) -> Attempt {
    let mut seen = false;
    for (pos, session) in friend.inbound_megolm.iter_mut().enumerate().rev() {
        match session.decrypt(message) {
            Ok(decrypted) => {
                // Тот же пакет больше не расшифруется: защита от повтора внутри сессии,
                // а старые индексы стираются — прямая секретность на стороне получателя.
                if let Some(next) = decrypted.message_index.checked_add(1) {
                    session.advance_to(next);
                }
                return Attempt::Decrypted {
                    pos,
                    plaintext: decrypted.plaintext,
                };
            }
            // vodozemac проверяет подпись раньше индекса: это сессия друга, пакет уже был.
            Err(DecryptionError::UnknownMessageIndex(..)) => seen = true,
            Err(_) => {}
        }
    }
    if seen {
        Attempt::AlreadySeen
    } else {
        Attempt::NotThisFriend
    }
}

fn accept_payload(friend: &mut Friend, id: AccountId, pos: usize, plaintext: &[u8]) -> Event {
    let Ok(payload) = LocationPayload::decode(plaintext) else {
        return Event::Dropped {
            reason: "malformed location payload",
        };
    };
    // Новая сессия заработала — старые удаляем, откатиться к ним нельзя (§7.5).
    // Вместе с advance_to(index + 1) это гарантирует, что ничего старше последнего
    // принятого пакета уже не расшифруется; `timestamp` — только для показа.
    friend.inbound_megolm.drain(..pos);
    Event::LocationUpdated {
        friend: id,
        payload,
    }
}

fn payload_for(
    precision: Precision,
    sharing: Sharing,
    location: Option<Location>,
    now: i64,
) -> LocationPayload {
    if sharing.ghost || precision == Precision::Hidden {
        return LocationPayload::hidden(now);
    }
    let (kind, loc, timestamp) = match (sharing.frozen, location) {
        (Some(frozen), _) => (LocationKind::Frozen, frozen, frozen.timestamp),
        (None, Some(loc)) => (LocationKind::Exact, loc, loc.timestamp),
        (None, None) => unreachable!("checked by the caller"),
    };
    let (lat_e7, lon_e7, accuracy_m, kind) = match precision {
        Precision::Approx => {
            let (lat, lon, acc) = snap_to_grid(loc.lat_e7, loc.lon_e7);
            let kind = if kind == LocationKind::Frozen {
                kind
            } else {
                LocationKind::Approx
            };
            (lat, lon, acc.max(loc.accuracy_m), kind)
        }
        Precision::Exact | Precision::Hidden => (loc.lat_e7, loc.lon_e7, loc.accuracy_m, kind),
    };
    LocationPayload {
        kind,
        lat_e7,
        lon_e7,
        accuracy_m,
        timestamp,
    }
}

/// Ротирует исходящую сессию, если пора (§7.5); возвращает `SessionShare` для отправки.
fn rotate_if_due(
    friend: &mut Friend,
    to: AccountId,
    now: i64,
) -> Result<Option<Outgoing>, CoreError> {
    let due = match &friend.outbound_megolm {
        None => true,
        Some((session, created_at)) => {
            session.message_index() >= MEGOLM_ROTATION_MESSAGES
                || now.saturating_sub(*created_at) >= MEGOLM_ROTATION_AGE.as_secs() as i64
        }
    };
    if !due {
        return Ok(None);
    }
    let session = GroupSession::new(MegolmConfig::version_1());
    let share = ControlMessage::SessionShare {
        session_key: SessionKeyBytes(session.session_key().to_bytes()),
    };
    friend.outbound_megolm = Some((session, now));
    encrypt_control(friend, to, &share).map(Some)
}
