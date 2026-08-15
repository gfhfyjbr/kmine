mod crypto;
mod keychain;
mod migrate;

use crate::error::EngineError;
use crate::ids::{AccountId, InstanceId, Loader};
use crate::types::{AccountRecord, InstanceRow};
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

    pub fn insert_instance(&self, row: &InstanceRow) -> Result<(), EngineError> {
        self.conn.execute(
            "INSERT INTO instances(
                id, slug, name, minecraft_version, loader, loader_version,
                account_uuid, memory_min_mb, memory_max_mb, jvm_flags, java_path,
                sandbox, icon_png, created_at, last_played_at, playtime_secs, session_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            rusqlite::params![
                row.id.as_hyphenated(),
                row.slug,
                row.name,
                row.minecraft_version,
                row.loader.as_str(),
                row.loader_version,
                row.account_uuid.as_ref().map(AccountId::as_hyphenated),
                row.memory_min_mb,
                row.memory_max_mb,
                row.jvm_flags,
                row.java_path,
                row.sandbox,
                row.icon_png,
                row.created_at,
                row.last_played_at,
                row.playtime_secs,
                row.session_count,
            ],
        )?;
        Ok(())
    }

    pub fn get_instance(&self, id: InstanceId) -> Result<Option<InstanceRow>, EngineError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, name, minecraft_version, loader, loader_version,
                    account_uuid, memory_min_mb, memory_max_mb, jvm_flags, java_path,
                    sandbox, icon_png, created_at, last_played_at, playtime_secs, session_count
             FROM instances WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id.as_hyphenated()])?;
        match rows.next()? {
            Some(row) => Ok(Some(instance_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_instances(&self) -> Result<Vec<InstanceRow>, EngineError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, name, minecraft_version, loader, loader_version,
                    account_uuid, memory_min_mb, memory_max_mb, jvm_flags, java_path,
                    sandbox, icon_png, created_at, last_played_at, playtime_secs, session_count
             FROM instances
             ORDER BY last_played_at DESC NULLS LAST, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], instance_from_row)?;
        let mut recs = Vec::new();
        for row in rows {
            recs.push(row?);
        }
        Ok(recs)
    }

    pub fn update_instance(&self, row: &InstanceRow) -> Result<(), EngineError> {
        self.conn.execute(
            "UPDATE instances SET
                slug = ?2,
                name = ?3,
                minecraft_version = ?4,
                loader = ?5,
                loader_version = ?6,
                account_uuid = ?7,
                memory_min_mb = ?8,
                memory_max_mb = ?9,
                jvm_flags = ?10,
                java_path = ?11,
                sandbox = ?12,
                icon_png = ?13,
                created_at = ?14,
                last_played_at = ?15,
                playtime_secs = ?16,
                session_count = ?17
             WHERE id = ?1",
            rusqlite::params![
                row.id.as_hyphenated(),
                row.slug,
                row.name,
                row.minecraft_version,
                row.loader.as_str(),
                row.loader_version,
                row.account_uuid.as_ref().map(AccountId::as_hyphenated),
                row.memory_min_mb,
                row.memory_max_mb,
                row.jvm_flags,
                row.java_path,
                row.sandbox,
                row.icon_png,
                row.created_at,
                row.last_played_at,
                row.playtime_secs,
                row.session_count,
            ],
        )?;
        Ok(())
    }

    pub fn delete_instance(&self, id: InstanceId) -> Result<(), EngineError> {
        self.conn
            .execute("DELETE FROM instances WHERE id = ?1", [id.as_hyphenated()])?;
        Ok(())
    }

    pub fn list_slugs(&self) -> Result<Vec<String>, EngineError> {
        let mut stmt = self.conn.prepare("SELECT slug FROM instances")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut slugs = Vec::new();
        for row in rows {
            slugs.push(row?);
        }
        Ok(slugs)
    }
}

fn loader_from_db(s: &str) -> Result<Loader, rusqlite::Error> {
    match s {
        "vanilla" => Ok(Loader::Vanilla),
        "fabric" => Ok(Loader::Fabric),
        "forge" => Ok(Loader::Forge),
        "neoforge" => Ok(Loader::NeoForge),
        "quilt" => Ok(Loader::Quilt),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, s)),
        )),
    }
}

fn parse_uuid_at(idx: usize, s: &str) -> Result<uuid::Uuid, rusqlite::Error> {
    uuid::Uuid::parse_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn instance_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstanceRow> {
    let id_str: String = row.get(0)?;
    let id = InstanceId(parse_uuid_at(0, &id_str)?);
    let loader_str: String = row.get(4)?;
    let loader = loader_from_db(&loader_str)?;
    let account_uuid = match row.get::<_, Option<String>>(6)? {
        Some(s) => Some(AccountId(parse_uuid_at(6, &s)?)),
        None => None,
    };
    Ok(InstanceRow {
        id,
        slug: row.get(1)?,
        name: row.get(2)?,
        minecraft_version: row.get(3)?,
        loader,
        loader_version: row.get(5)?,
        account_uuid,
        memory_min_mb: row.get(7)?,
        memory_max_mb: row.get(8)?,
        jvm_flags: row.get(9)?,
        java_path: row.get(10)?,
        sandbox: row.get(11)?,
        icon_png: row.get(12)?,
        created_at: row.get(13)?,
        last_played_at: row.get(14)?,
        playtime_secs: row.get(15)?,
        session_count: row.get(16)?,
    })
}

#[cfg(test)]
mod tests {
    use super::Store;
    use super::keychain::MemoryKeychain;
    use crate::ids::{AccountId, InstanceId, Loader};
    use crate::types::{AccountRecord, InstanceRow};
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

    #[test]
    fn insert_and_list_instance() {
        let kc = MemoryKeychain::new();
        let (store, _) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let id = InstanceId::new();
        store
            .insert_instance(&InstanceRow {
                id,
                slug: "A".into(),
                name: "A".into(),
                minecraft_version: "1.21.1".into(),
                loader: Loader::Vanilla,
                loader_version: None,
                account_uuid: None,
                memory_min_mb: None,
                memory_max_mb: Some(4096),
                jvm_flags: None,
                java_path: None,
                sandbox: false,
                icon_png: None,
                created_at: 1,
                last_played_at: None,
                playtime_secs: 0,
                session_count: 0,
            })
            .unwrap();
        let list = store.list_instances().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].minecraft_version, "1.21.1");
    }
}
