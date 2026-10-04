-- Задача 3.2: аккаунты, challenge для входа, сессии (protocol §4.1, §4.2).
-- Сервер знает только случайный ID аккаунта и его публичные ключи.

CREATE TABLE accounts (
    account_id bytea PRIMARY KEY CHECK (length(account_id) = 16),
    ik         bytea NOT NULL CHECK (length(ik) = 32),
    sk         bytea NOT NULL CHECK (length(sk) = 32),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Одноразовый nonce, живёт 60 с.
CREATE TABLE auth_challenges (
    nonce      bytea PRIMARY KEY CHECK (length(nonce) = 32),
    account_id bytea NOT NULL REFERENCES accounts ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE INDEX auth_challenges_expires ON auth_challenges (expires_at);

-- Хранится только SHA-256 токена; сам токен знает лишь клиент.
CREATE TABLE sessions (
    token_hash bytea PRIMARY KEY CHECK (length(token_hash) = 32),
    account_id bytea NOT NULL REFERENCES accounts ON DELETE CASCADE,
    expires_at timestamptz NOT NULL
);
CREATE INDEX sessions_expires ON sessions (expires_at);
CREATE INDEX sessions_account ON sessions (account_id);
