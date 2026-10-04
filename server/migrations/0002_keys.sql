-- Задача 3.3: каталог одноразовых (OTK) и fallback-ключей (protocol §3, §4.3).
-- Ключи публичные; подписи проверяет и сервер (отсев мусора), и получатель.

CREATE TABLE one_time_keys (
    id         bigserial PRIMARY KEY,
    account_id bytea NOT NULL REFERENCES accounts ON DELETE CASCADE,
    key        bytea NOT NULL CHECK (length(key) = 32),
    signature  bytea NOT NULL CHECK (length(signature) = 64),
    UNIQUE (account_id, key)
);
CREATE INDEX one_time_keys_account ON one_time_keys (account_id, id);

-- Один текущий fallback-ключ на аккаунт; новый заменяет старый.
CREATE TABLE fallback_keys (
    account_id bytea PRIMARY KEY REFERENCES accounts ON DELETE CASCADE,
    key        bytea NOT NULL CHECK (length(key) = 32),
    signature  bytea NOT NULL CHECK (length(signature) = 64)
);
