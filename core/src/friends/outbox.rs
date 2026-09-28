//! Исходящая очередь (`docs/protocol.md` §8.2, п. 4).
//!
//! Всё, что ядро хочет отправить, попадает сюда в той же транзакции, что и
//! изменение состояния, и удаляется только после подтверждения сервером.
//! Так ответ не теряется, если приложение закрылось или сеть отвалилась.

use serde::{Deserialize, Serialize};
use staya_proto::AccountId;
use staya_proto::api::EnvelopeKind;

use super::Outgoing;

pub(super) const OUTBOX_RECORD: &str = "outbox";

/// Конверт, ожидающий отправки.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedEnvelope {
    /// Номер для [`super::Friends::mark_sent`].
    pub id: u64,
    pub to: AccountId,
    pub kind: EnvelopeKind,
    pub data: Vec<u8>,
}

/// Что платформе нужно отправить на сервер.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PendingSends {
    /// Сначала все управляющие по порядку, затем позиции (§8.2): так `SessionShare`
    /// всегда опережает пакеты новой сессии, даже если прошлая отправка сорвалась.
    pub envelopes: Vec<QueuedEnvelope>,
    /// Получатели, у которых нужно удалить наш слот позиции (§9).
    ///
    /// Правило для платформы: удаления выполняются **раньше** конвертов, и если
    /// хотя бы одно не удалось, конверты в этой попытке не отправляются. Иначе
    /// удаление, прошедшее позже, стёрло бы уже новый пакет повторно добавленного друга.
    pub delete_slots: Vec<AccountId>,
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Outbox {
    next_id: u64,
    /// Все управляющие, по порядку.
    control: Vec<Item>,
    /// Только последняя позиция для каждого получателя.
    locations: Vec<Item>,
    delete_slots: Vec<[u8; 16]>,
}

#[derive(Serialize, Deserialize)]
struct Item {
    id: u64,
    to: [u8; 16],
    data: Vec<u8>,
}

impl Outbox {
    pub(super) fn push(&mut self, out: Outgoing) {
        let item = Item {
            id: self.next_id,
            to: out.to.0,
            data: out.data,
        };
        self.next_id += 1;
        match out.kind {
            EnvelopeKind::Control => self.control.push(item),
            EnvelopeKind::Location => {
                // Старая позиция для этого получателя больше не нужна.
                self.locations.retain(|i| i.to != item.to);
                self.locations.push(item);
            }
        }
    }

    pub(super) fn pending(&self) -> PendingSends {
        let queued = |kind: EnvelopeKind| {
            move |i: &Item| QueuedEnvelope {
                id: i.id,
                to: AccountId(i.to),
                kind,
                data: i.data.clone(),
            }
        };
        PendingSends {
            envelopes: self
                .control
                .iter()
                .map(queued(EnvelopeKind::Control))
                .chain(self.locations.iter().map(queued(EnvelopeKind::Location)))
                .collect(),
            delete_slots: self.delete_slots.iter().map(|id| AccountId(*id)).collect(),
        }
    }

    pub(super) fn mark_sent(&mut self, ids: &[u64]) {
        self.control.retain(|i| !ids.contains(&i.id));
        self.locations.retain(|i| !ids.contains(&i.id));
    }

    /// Отменяет всё неотправленное этому получателю (друг удалён).
    pub(super) fn drop_for(&mut self, to: &AccountId) {
        self.control.retain(|i| i.to != to.0);
        self.locations.retain(|i| i.to != to.0);
    }

    pub(super) fn request_slot_deletion(&mut self, recipient: &AccountId) {
        if !self.delete_slots.contains(&recipient.0) {
            self.delete_slots.push(recipient.0);
        }
    }

    pub(super) fn mark_slot_deleted(&mut self, recipient: &AccountId) {
        self.delete_slots.retain(|id| *id != recipient.0);
    }
}
