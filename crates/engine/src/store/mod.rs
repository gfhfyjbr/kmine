mod crypto;
mod keychain;
mod migrate;

use crate::error::EngineError;
use crate::ids::AccountId;
use crate::types::AccountRecord;
use rusqlite::Connection;
use std::path::Path;

pub use keychain::{Keychain, MemoryKeychain, OsKeychain};

pub fn ensure_master_key(keychain: &dyn Keychain) -> Result<[u8; 32], EngineError> {
    if let Some(key) = keychain.get_master_key()? {
        Ok(key)
    } else {
        let key = crypto::generate_master_key();
        keychain.set_master_key(&key)?;
        Ok(key)
    }
}

pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    pub fn open(path: &Path, keychain: &dyn Keychain) -> Result<(Self, [u8; 32]), EngineError> {
        let store = Self::open_file(path)?;
        let key = ensure_master_key(keychain)?;
        Ok((store, key))
    }

    pub fn open_file(path: &Path) -> Result<Self, EngineError> {
        let conn = if path.as_os_str() == ":" || path == Path::new(":memory:") {
            Connection::open_in_memory()?
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
            }
            Connection::open(path)?
        };
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        migrate::migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn migrate(&self) -> Result<(), EngineError> {
        migrate::migrate(&self.conn)
    }

    pub fn get_config(&self, key: &str) -> Result<Option<String>, EngineError> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM config WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn set_config(&self, key: &str, value: &str) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT INTO config(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )?;
        Ok(())
    }

    pub fn put_secret(
        &self,
        key: &[u8; 32],
        id: &str,
        plaintext: &[u8],
    ) -> Result<(), EngineError> {
        let (nonce, ciphertext) = crypto::seal(key, id, plaintext)?;
        self.conn.execute(
            "INSERT INTO secrets(id, nonce, ciphertext) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                nonce = excluded.nonce,
                ciphertext = excluded.ciphertext",
            rusqlite::params![id, nonce, ciphertext],
        )?;
        Ok(())
    }

    pub fn get_secret(&self, key: &[u8; 32], id: &str) -> Result<Option<Vec<u8>>, EngineError> {
        let mut stmt = self
            .conn
            .prepare("SELECT nonce, ciphertext FROM secrets WHERE id = ?1")?;
        let mut rows = stmt.query([id])?;
        match rows.next()? {
            Some(row) => {
                let nonce: Vec<u8> = row.get(0)?;
                let ciphertext: Vec<u8> = row.get(1)?;
                Ok(Some(crypto::open(key, id, &nonce, &ciphertext)?))
            }
            None => Ok(None),
        }
    }

    pub fn delete_secret(&self, id: &str) -> Result<(), EngineError> {
        self.conn
            .execute("DELETE FROM secrets WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn upsert_account(&self, rec: &AccountRecord) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT INTO accounts(uuid, username, added_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(uuid) DO UPDATE SET username = excluded.username, last_used_at = excluded.last_used_at",
            rusqlite::params![
                rec.uuid.as_hyphenated(),
                rec.username,
                rec.added_at,
                rec.last_used_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_accounts(&self) -> Result<Vec<AccountRecord>, EngineError> {
        let mut stmt = self.conn.prepare(
            "SELECT uuid, username, added_at, last_used_at FROM accounts ORDER BY added_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let uuid_str: String = row.get(0)?;
            let uuid = uuid::Uuid::parse_str(&uuid_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(AccountRecord {
                uuid: AccountId(uuid),
                username: row.get(1)?,
                added_at: row.get(2)?,
                last_used_at: row.get(3)?,
            })
        })?;
        let mut recs = Vec::new();
        for row in rows {
            recs.push(row?);
        }
        Ok(recs)
    }

    pub fn delete_account(&self, id: AccountId) -> Result<(), EngineError> {
        let uuid = id.as_hyphenated();
        let secret_id = format!("account/{uuid}");
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM secrets WHERE id = ?1", [&secret_id])?;
        tx.execute("DELETE FROM accounts WHERE uuid = ?1", [&uuid])?;
        tx.commit()?;
        Ok(())
    }

    pub fn selected_account(&self) -> Result<Option<AccountId>, EngineError> {
        let Some(raw) = self.get_config("selected_account")? else {
            return Ok(None);
        };
        serde_json::from_str(&raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
                .into()
        })
    }

    pub fn set_selected_account(&self, id: Option<AccountId>) -> Result<(), EngineError> {
        let value = match id {
            Some(id) => format!("\"{}\"", id.as_hyphenated()),
            None => "null".into(),
        };
        self.set_config("selected_account", &value)
    }
}

#[cfg(test)]
mod tests {
    use super::Store;
    use super::keychain::MemoryKeychain;
    use crate::ids::AccountId;
    use crate::types::AccountRecord;
    use std::path::Path;

    fn open_mem() -> Store {
        Store::open_file(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn migrate_sets_user_version_1() {
        let store = open_mem();
        let v: i32 = store
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn config_round_trip() {
        let store = open_mem();
        assert_eq!(store.get_config("selected_account").unwrap(), None);
        store.set_config("selected_account", "\"abc\"").unwrap();
        assert_eq!(
            store.get_config("selected_account").unwrap().as_deref(),
            Some("\"abc\"")
        );
        store
            .set_config("window", r#"{"width":800,"height":600}"#)
            .unwrap();
        assert!(store.get_config("window").unwrap().unwrap().contains("800"));
    }

    #[test]
    fn migrate_is_idempotent() {
        let store = open_mem();
        store.migrate().unwrap();
        store.migrate().unwrap();
        let v: i32 = store
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn put_get_secret_round_trip() {
        let kc = MemoryKeychain::new();
        let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        store
            .put_secret(&key, "account/u1", br#"{"msa_refresh":"r"}"#)
            .unwrap();
        let got = store.get_secret(&key, "account/u1").unwrap().unwrap();
        assert_eq!(got, br#"{"msa_refresh":"r"}"#);
    }

    #[test]
    fn open_creates_master_key_once() {
        let kc = MemoryKeychain::new();
        let (_, k1) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let (_, k2) = Store::open(Path::new(":memory:"), &kc).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn missing_secret_is_none() {
        let kc = MemoryKeychain::new();
        let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        assert!(store.get_secret(&key, "nope").unwrap().is_none());
    }

    #[test]
    fn upsert_list_select_delete_account() {
        let kc = MemoryKeychain::new();
        let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let id = AccountId(uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap());
        store
            .upsert_account(&AccountRecord {
                uuid: id,
                username: "Steve".into(),
                added_at: 10,
                last_used_at: None,
            })
            .unwrap();
        store
            .put_secret(&key, &format!("account/{}", id.as_hyphenated()), b"{}")
            .unwrap();
        store.set_selected_account(Some(id)).unwrap();
        assert_eq!(store.list_accounts().unwrap().len(), 1);
        assert_eq!(store.selected_account().unwrap(), Some(id));
        store.delete_account(id).unwrap();
        assert!(store.list_accounts().unwrap().is_empty());
        assert!(
            store
                .get_secret(&key, &format!("account/{}", id.as_hyphenated()))
                .unwrap()
                .is_none()
        );
        assert_eq!(store.selected_account().unwrap(), Some(id)); // config left as-is; Engine clears it in Task 7
    }

    #[test]
    fn delete_account_sets_nothing_if_missing() {
        let kc = MemoryKeychain::new();
        let (store, _key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let id = AccountId(uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap());
        store.delete_account(id).unwrap();
    }
}
