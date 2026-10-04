//! Правила почтовых ящиков (protocol §8.1) и их реализация в памяти для
//! dev-сервера (задача 2.10). Реализация в PostgreSQL — `envelopes.rs` (3.4).
//! Сервер не знает ничего о содержимом конвертов, кроме размера.

use std::collections::{BTreeMap, VecDeque};

use staya_proto::AccountId;
use staya_proto::api::{
    ClaimResponse, EnvelopeKind, EnvelopeStatus, LocationSlot, MailboxResponse, PublishKeysRequest,
    QueuedControl, SendRequest, SendResponse, SignedKey,
};
use staya_proto::consts::{CONTROL_QUEUE_LIMIT, LOCATION_ENVELOPE_LEN, is_control_envelope_len};

#[derive(Default)]
pub struct Mailbox {
    otks: BTreeMap<AccountId, Vec<SignedKey>>,
    fallback: BTreeMap<AccountId, SignedKey>,
    control: BTreeMap<AccountId, VecDeque<QueuedControl>>,
    /// Последний конверт позиции для пары (отправитель, получатель).
    slots: BTreeMap<(AccountId, AccountId), Vec<u8>>,
    next_seq: i64,
}

impl Mailbox {
    /// Аккаунт известен серверу (в dev — опубликовал ключи).
    pub fn knows(&self, id: &AccountId) -> bool {
        self.otks.contains_key(id) || self.fallback.contains_key(id)
    }

    /// `PUT /v1/keys`. Подписи проверяет получатель ключа (protocol §4.3).
    pub fn publish_keys(&mut self, who: AccountId, req: PublishKeysRequest) {
        self.otks.entry(who).or_default().extend(req.one_time_keys);
        if let Some(fb) = req.fallback_key {
            self.fallback.insert(who, fb);
        }
    }

    /// `POST /v1/keys/claim`: один OTK, а когда они кончились — fallback-ключ.
    pub fn claim(&mut self, target: &AccountId) -> Option<ClaimResponse> {
        if let Some(key) = self.otks.get_mut(target).and_then(Vec::pop) {
            return Some(ClaimResponse {
                key,
                is_fallback: false,
            });
        }
        self.fallback.get(target).map(|key| ClaimResponse {
            key: key.clone(),
            is_fallback: true,
        })
    }

    pub fn otk_count(&self, who: &AccountId) -> u32 {
        self.otks
            .get(who)
            .map_or(0, |k| u32::try_from(k.len()).unwrap_or(u32::MAX))
    }

    /// `POST /v1/envelopes`: статус каждого конверта; плохой не отклоняет остальные.
    pub fn send(&mut self, from: AccountId, req: SendRequest) -> SendResponse {
        let results = req
            .envelopes
            .into_iter()
            .map(|env| {
                if !size_ok(env.kind, env.data.0.len()) {
                    return rejected(BAD_SIZE);
                }
                if !self.knows(&env.to) {
                    return rejected(UNKNOWN_ACCOUNT);
                }
                match env.kind {
                    EnvelopeKind::Location => {
                        self.slots.insert((from, env.to), env.data.0);
                    }
                    EnvelopeKind::Control => {
                        let queue = self.control.entry(env.to).or_default();
                        if queue.len() >= CONTROL_QUEUE_LIMIT {
                            return rejected(QUEUE_FULL);
                        }
                        self.next_seq += 1;
                        queue.push_back(QueuedControl {
                            seq: self.next_seq,
                            from,
                            data: env.data,
                        });
                    }
                }
                EnvelopeStatus::Accepted
            })
            .collect();
        SendResponse { results }
    }

    /// `GET /v1/mailbox`: вся очередь и все слоты для получателя.
    pub fn mailbox(&self, who: &AccountId) -> MailboxResponse {
        let control = self
            .control
            .get(who)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default();
        let locations = self
            .slots
            .iter()
            .filter(|((_, to), _)| to == who)
            .map(|((from, _), data)| LocationSlot {
                from: *from,
                data: staya_proto::api::B64(data.clone()),
            })
            .collect();
        MailboxResponse { control, locations }
    }

    /// `POST /v1/mailbox/ack`. Чужие номера ничего не удаляют.
    pub fn ack(&mut self, who: &AccountId, seqs: &[i64]) {
        if let Some(q) = self.control.get_mut(who) {
            q.retain(|c| !seqs.contains(&c.seq));
        }
    }

    /// `DELETE /v1/slots/{recipient}`: идемпотентно.
    pub fn delete_slot(&mut self, from: AccountId, to: AccountId) {
        self.slots.remove(&(from, to));
    }
}

/// Причины окончательного отказа (protocol §8.2.5): одинаковы в памяти и в Postgres.
pub const BAD_SIZE: &str = "bad size";
pub const UNKNOWN_ACCOUNT: &str = "unknown account";
pub const QUEUE_FULL: &str = "queue full";

/// Конверт точного размера для своего вида (§8.1).
pub fn size_ok(kind: EnvelopeKind, len: usize) -> bool {
    match kind {
        EnvelopeKind::Location => len == LOCATION_ENVELOPE_LEN,
        EnvelopeKind::Control => is_control_envelope_len(len),
    }
}

pub fn rejected(reason: &str) -> EnvelopeStatus {
    EnvelopeStatus::Rejected {
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use staya_proto::api::{B64, OutgoingEnvelope};

    fn id(n: u8) -> AccountId {
        AccountId([n; 16])
    }

    fn known(mb: &mut Mailbox, who: AccountId) {
        let key = SignedKey {
            key: B64(vec![1; 32]),
            signature: B64(vec![2; 64]),
        };
        mb.publish_keys(
            who,
            PublishKeysRequest {
                one_time_keys: vec![key.clone()],
                fallback_key: Some(key),
            },
        );
    }

    fn env(to: AccountId, kind: EnvelopeKind, len: usize) -> OutgoingEnvelope {
        OutgoingEnvelope {
            to,
            kind,
            data: B64(vec![7; len]),
        }
    }

    fn send(mb: &mut Mailbox, from: AccountId, envs: Vec<OutgoingEnvelope>) -> Vec<EnvelopeStatus> {
        mb.send(from, SendRequest { envelopes: envs }).results
    }

    #[test]
    fn location_slot_is_upserted_and_control_is_queued() {
        let mut mb = Mailbox::default();
        known(&mut mb, id(2));
        let r = send(
            &mut mb,
            id(1),
            vec![
                env(id(2), EnvelopeKind::Location, LOCATION_ENVELOPE_LEN),
                env(id(2), EnvelopeKind::Control, 512),
                env(id(2), EnvelopeKind::Location, LOCATION_ENVELOPE_LEN),
            ],
        );
        assert!(r.iter().all(|s| *s == EnvelopeStatus::Accepted));
        let m = mb.mailbox(&id(2));
        assert_eq!(m.locations.len(), 1);
        assert_eq!(m.locations[0].from, id(1));
        assert_eq!(m.control.len(), 1);
        // Позиция не вытесняет управляющее сообщение.
        assert_eq!(m.control[0].data.0.len(), 512);
    }

    #[test]
    fn bad_envelopes_are_rejected_individually() {
        let mut mb = Mailbox::default();
        known(&mut mb, id(2));
        let r = send(
            &mut mb,
            id(1),
            vec![
                env(id(2), EnvelopeKind::Location, 161),
                env(id(2), EnvelopeKind::Control, 513),
                env(id(9), EnvelopeKind::Control, 512),
                env(id(2), EnvelopeKind::Control, 1280),
            ],
        );
        assert!(matches!(r[0], EnvelopeStatus::Rejected { .. }));
        assert!(matches!(r[1], EnvelopeStatus::Rejected { .. }));
        assert!(matches!(r[2], EnvelopeStatus::Rejected { .. }));
        assert_eq!(r[3], EnvelopeStatus::Accepted);
    }

    #[test]
    fn queue_limit_and_ack() {
        let mut mb = Mailbox::default();
        known(&mut mb, id(2));
        let envs = (0..=CONTROL_QUEUE_LIMIT)
            .map(|_| env(id(2), EnvelopeKind::Control, 512))
            .collect();
        let r = send(&mut mb, id(1), envs);
        assert_eq!(
            r.iter().filter(|s| **s == EnvelopeStatus::Accepted).count(),
            CONTROL_QUEUE_LIMIT
        );
        assert!(matches!(r.last(), Some(EnvelopeStatus::Rejected { .. })));
        let seqs: Vec<i64> = mb.mailbox(&id(2)).control.iter().map(|c| c.seq).collect();
        // Подтверждение чужой очереди ничего не удаляет.
        mb.ack(&id(3), &seqs);
        assert_eq!(mb.mailbox(&id(2)).control.len(), CONTROL_QUEUE_LIMIT);
        mb.ack(&id(2), &seqs[..10]);
        assert_eq!(mb.mailbox(&id(2)).control.len(), CONTROL_QUEUE_LIMIT - 10);
    }

    #[test]
    fn claim_falls_back_and_slot_delete_is_idempotent() {
        let mut mb = Mailbox::default();
        known(&mut mb, id(2));
        assert_eq!(mb.otk_count(&id(2)), 1);
        assert!(!mb.claim(&id(2)).unwrap().is_fallback);
        assert!(mb.claim(&id(2)).unwrap().is_fallback);
        assert!(mb.claim(&id(5)).is_none());

        send(
            &mut mb,
            id(1),
            vec![env(id(2), EnvelopeKind::Location, LOCATION_ENVELOPE_LEN)],
        );
        mb.delete_slot(id(1), id(2));
        mb.delete_slot(id(1), id(2));
        assert!(mb.mailbox(&id(2)).locations.is_empty());
    }
}
