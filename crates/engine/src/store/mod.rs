mod crypto;
mod keychain;
mod migrate;

use crate::error::EngineError;
use rusqlite::Connection;
use std::path::Path;

pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
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
}

#[cfg(test)]
mod tests {
    use super::Store;

    fn open_mem() -> Store {
        Store::open_file(std::path::Path::new(":memory:")).unwrap()
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
}
