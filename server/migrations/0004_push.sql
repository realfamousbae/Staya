-- Opaque push addresses only; message contents remain E2E encrypted.
CREATE TABLE push_devices (
    account_id BYTEA PRIMARY KEY REFERENCES accounts(account_id) ON DELETE CASCADE,
    token TEXT NOT NULL CHECK (octet_length(token) BETWEEN 16 AND 4096),
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
