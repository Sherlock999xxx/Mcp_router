-- sqlx migration
ALTER TABLE users ADD COLUMN name TEXT;
ALTER TABLE api_tokens ADD COLUMN created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'));

CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    description TEXT,
    created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE TABLE IF NOT EXISTS provider_keys (
    provider_id TEXT NOT NULL,
    name TEXT NOT NULL,
    encrypted_value BLOB NOT NULL,
    nonce BLOB NOT NULL,
    created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (provider_id, name),
    FOREIGN KEY(provider_id) REFERENCES providers(id)
);

CREATE TABLE IF NOT EXISTS upstreams (
    name TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    command TEXT,
    args TEXT,
    url TEXT,
    bearer TEXT,
    provider_slug TEXT
);
