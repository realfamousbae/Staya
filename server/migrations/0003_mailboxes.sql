-- Задача 3.4: почтовые ящики (protocol §8.1). Содержимое — только шифротекст
-- фиксированного размера; сервер знает отправителя, получателя и время.

-- Очередь управляющих сообщений до подтверждения; хранение 30 дней.
CREATE TABLE control_queue (
    seq        bigserial PRIMARY KEY,
    recipient  bytea NOT NULL REFERENCES accounts ON DELETE CASCADE,
    sender     bytea NOT NULL REFERENCES accounts ON DELETE CASCADE,
    data       bytea NOT NULL CHECK (length(data) IN (512, 1280, 9472)),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX control_queue_recipient ON control_queue (recipient, seq);
CREATE INDEX control_queue_created ON control_queue (created_at);

-- Последний конверт позиции для пары (отправитель, получатель); хранение 72 часа.
CREATE TABLE location_slots (
    sender     bytea NOT NULL REFERENCES accounts ON DELETE CASCADE,
    recipient  bytea NOT NULL REFERENCES accounts ON DELETE CASCADE,
    data       bytea NOT NULL CHECK (length(data) = 160),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (sender, recipient)
);
CREATE INDEX location_slots_recipient ON location_slots (recipient);
CREATE INDEX location_slots_updated ON location_slots (updated_at);
