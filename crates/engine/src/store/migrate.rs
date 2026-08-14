use crate::error::EngineError;
use rusqlite::Connection;

pub const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS accounts (
    uuid         TEXT PRIMARY KEY,
    username     TEXT NOT NULL,
    added_at     INTEGER NOT NULL,
    last_used_at INTEGER
);
CREATE TABLE IF NOT EXISTS secrets (
    id         TEXT PRIMARY KEY,
    nonce      BLOB NOT NULL,
    ciphertext BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS instances (
    id                TEXT PRIMARY KEY,
    slug              TEXT NOT NULL UNIQUE,
    name              TEXT NOT NULL,
    minecraft_version TEXT NOT NULL,
    loader            TEXT NOT NULL,
    loader_version    TEXT,
    account_uuid      TEXT,
    memory_min_mb     INTEGER,
    memory_max_mb     INTEGER,
    jvm_flags         TEXT,
    java_path         TEXT,
    sandbox           INTEGER NOT NULL DEFAULT 0,
    icon_png          BLOB,
    created_at        INTEGER NOT NULL,
    last_played_at    INTEGER,
    playtime_secs     INTEGER NOT NULL DEFAULT 0,
    session_count     INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (account_uuid) REFERENCES accounts(uuid) ON DELETE SET NULL
);
"#;

pub fn migrate(conn: &Connection) -> Result<(), EngineError> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 1 {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(SCHEMA_V1)?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }
    Ok(())
}
