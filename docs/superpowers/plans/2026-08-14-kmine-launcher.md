# kmine Launcher Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a from-scratch GPUI Minecraft launcher that signs in with Microsoft, manages instances in `kmine.db`, downloads Java/game files, and launches vanilla, Fabric, or Forge — including opt-in OS sandbox.

**Architecture:** Two packages: `kmine` (GPUI binary) and `crates/engine` (`kmine-engine`, no gpui). UI talks to `EngineHandle` via commands/events. Launch is `prepare() -> LaunchPlan` then `spawn()`. Persistence is SQLite `kmine.db`; account tokens are AES-256-GCM sealed with a 32-byte master key that lives only in the OS keychain.

**Tech Stack:** Rust edition 2024, gpui + gpui-component, tokio, rusqlite (bundled), aes-gcm, reqwest, oauth2, sha1, zip, wiremock (dev).

**Spec:** `docs/superpowers/specs/2026-08-14-kmine-launcher-design.md`

## Global Constraints

- Rust edition **2024**.
- Workspace members: `.` (`kmine`) and `crates/engine` (`kmine-engine`). No extra crates (`schema`, `bridge`, `auth`).
- Database file name is exactly `kmine.db`.
- OAuth bind: `127.0.0.1:47821` / path `/auth`.
- Keychain: service `dev.kmine.launcher`, account `master-key`.
- Tokens never written plaintext to disk or logs.
- Own Microsoft `CLIENT_ID` only (constant may be empty; then `AuthNotConfigured`).
- Do not vendor or copy PandoraLauncher source. Public Mojang/Microsoft/Fabric/Forge protocols only.
- Engine tests never start GPUI. Live Mojang tests are `#[ignore]`.
- `launcher_name` = `kmine`. `launcher_version` = `CARGO_PKG_VERSION`.
- Content disable suffix is exactly `.disabled`. Ignore dotfiles. One directory level.
- After every task: `cargo test -p kmine-engine` is green.

## File structure

| Path | Responsibility |
|---|---|
| `Cargo.toml` | Workspace + `kmine` binary deps |
| `crates/engine/Cargo.toml` | `kmine-engine` library |
| `crates/engine/src/lib.rs` | Re-exports, `Engine`, `EngineHandle`, `Event`, `Command` |
| `crates/engine/src/ids.rs` | `InstanceId`, `AccountId`, `Loader` |
| `crates/engine/src/error.rs` | `EngineError` |
| `crates/engine/src/paths.rs` | `LauncherPaths` |
| `crates/engine/src/types.rs` | `CreateInstance`, `InstancePatch`, summaries, content, quick play, `ProgressSink`, `LaunchPlan`, `SandboxSpec` |
| `crates/engine/src/store/mod.rs` | `Store`, config/accounts/instances/secrets CRUD |
| `crates/engine/src/store/migrate.rs` | `PRAGMA user_version` v1 |
| `crates/engine/src/store/crypto.rs` | AES-256-GCM seal/open |
| `crates/engine/src/store/keychain.rs` | `Keychain` trait, `MemoryKeychain`, OS keychain |
| `crates/engine/src/http.rs` | Download + sha1 |
| `crates/engine/src/auth/mod.rs` | Login + refresh chain |
| `crates/engine/src/auth/constants.rs` | `CLIENT_ID`, URLs, bind address |
| `crates/engine/src/java/mod.rs` | Resolve custom Java + Mojang runtime |
| `crates/engine/src/mojang/mod.rs` | Manifest, version JSON, rules, assets, libraries |
| `crates/engine/src/mojang/args.rs` | Argv substitution |
| `crates/engine/src/fabric/mod.rs` | Fabric merge |
| `crates/engine/src/forge/mod.rs` | Forge installer merge + processors |
| `crates/engine/src/instance/mod.rs` | Slug + folder create/rename/delete |
| `crates/engine/src/content.rs` | Local mods/resourcepacks/shaderpacks |
| `crates/engine/src/nbt.rs` | `servers.dat` + `level.dat` |
| `crates/engine/src/launch/mod.rs` | `prepare` + `spawn` |
| `crates/engine/src/redact.rs` | Token redaction |
| `crates/engine/src/sandbox/mod.rs` | `SandboxStatus`, fill spec, dispatch |
| `crates/engine/src/sandbox/macos.rs` | Seatbelt |
| `crates/engine/src/sandbox/linux.rs` | bwrap |
| `crates/engine/src/sandbox/windows.rs` | AppContainer |
| `crates/engine/tests/fixtures/` | version JSON, nbt, fabric/forge samples |
| `src/main.rs` | Tokio + `gpui_component::init` + window |
| `src/app.rs` | Root UI state, `EngineHandle` |
| `src/screens/*.rs` | Sidebar, Play, Content, Settings |
| `src/modals/*.rs` | Create, Accounts, Progress |
| `src/game_output.rs` | Second window |

---

### Task 1: Workspace, IDs, errors, paths

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/engine/Cargo.toml`
- Create: `crates/engine/src/lib.rs`
- Create: `crates/engine/src/ids.rs`
- Create: `crates/engine/src/error.rs`
- Create: `crates/engine/src/paths.rs`
- Test: `crates/engine/src/paths.rs` (`#[cfg(test)]`)
- Test: `crates/engine/src/ids.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: existing root `Cargo.toml` (gpui deps stay on the binary)
- Produces:
  - `pub struct InstanceId(pub Uuid)` with `from_uuid`, `as_hyphenated() -> String`
  - `pub struct AccountId(pub Uuid)` same
  - `pub enum Loader { Vanilla, Fabric, Forge }` serde `rename_all = "lowercase"`
  - `pub enum EngineError { ... }` (all spec variants; unused ones may wrap `String` sources for now)
  - `pub struct LauncherPaths { root, db, instances, cache_meta, cache_libraries, cache_assets_indexes, cache_assets_objects, cache_assets_virtual, cache_runtime, cache_natives }`
  - `impl LauncherPaths { pub fn new(root: PathBuf) -> Self; pub fn create_dirs(&self) -> Result<(), EngineError>; pub fn instance_minecraft(slug: &str) -> PathBuf; pub fn default_root() -> PathBuf }`

- [ ] **Step 1: Convert the root crate into a workspace and add the engine package**

`Cargo.toml`:

```toml
[workspace]
members = [".", "crates/engine"]
resolver = "3"

[workspace.package]
edition = "2024"
version = "0.1.0"

[package]
name = "kmine"
version.workspace = true
edition.workspace = true

[dependencies]
kmine-engine = { path = "crates/engine" }
gpui = { git = "https://github.com/zed-industries/zed" }
gpui_platform = { git = "https://github.com/zed-industries/zed", features = ["font-kit"] }
gpui-component = { git = "https://github.com/longbridge/gpui-component" }
```

`crates/engine/Cargo.toml`:

```toml
[package]
name = "kmine-engine"
version.workspace = true
edition.workspace = true

[dependencies]
directories = "6"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uuid = { version = "1", features = ["serde", "v4"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write failing tests for `Loader` parse and `LauncherPaths`**

`crates/engine/src/ids.rs` — write the test module first against public types that do not exist yet:

```rust
#[cfg(test)]
mod tests {
    use super::Loader;

    #[test]
    fn loader_serde_lowercase() {
        assert_eq!(serde_json::from_str::<Loader>("\"vanilla\"").unwrap(), Loader::Vanilla);
        assert_eq!(serde_json::from_str::<Loader>("\"fabric\"").unwrap(), Loader::Fabric);
        assert_eq!(serde_json::from_str::<Loader>("\"forge\"").unwrap(), Loader::Forge);
        assert_eq!(serde_json::to_string(&Loader::Forge).unwrap(), "\"forge\"");
    }
}
```

`crates/engine/src/paths.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::LauncherPaths;
    use std::path::PathBuf;

    #[test]
    fn paths_use_kmine_db_name() {
        let paths = LauncherPaths::new(PathBuf::from("/tmp/kmine-test"));
        assert_eq!(paths.db.file_name().unwrap(), "kmine.db");
        assert!(paths.instances.ends_with("instances"));
        assert!(paths.cache_runtime.ends_with("cache/runtime") || paths.cache_runtime.ends_with("cache\\runtime"));
    }

    #[test]
    fn create_dirs_makes_instance_and_cache_trees() {
        let dir = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(dir.path().to_path_buf());
        paths.create_dirs().unwrap();
        assert!(paths.instances.is_dir());
        assert!(paths.cache_meta.is_dir());
        assert!(paths.cache_libraries.is_dir());
        assert!(paths.cache_assets_indexes.is_dir());
        assert!(paths.cache_assets_objects.is_dir());
        assert!(paths.cache_assets_virtual.is_dir());
        assert!(paths.cache_runtime.is_dir());
        assert!(paths.cache_natives.is_dir());
    }

    #[test]
    fn instance_minecraft_is_under_slug() {
        let paths = LauncherPaths::new(PathBuf::from("/tmp/kmine-test"));
        let mc = paths.instance_minecraft("My Pack");
        assert!(mc.ends_with("instances/My Pack/.minecraft") || mc.ends_with("instances\\My Pack\\.minecraft"));
    }
}
```

`crates/engine/src/lib.rs`:

```rust
pub mod error;
pub mod ids;
pub mod paths;

pub use error::EngineError;
pub use ids::{AccountId, InstanceId, Loader};
pub use paths::LauncherPaths;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p kmine-engine`

Expected: FAIL compiling (`Loader` / `LauncherPaths` missing).

- [ ] **Step 4: Implement IDs, errors, paths**

`crates/engine/src/ids.rs`:

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(pub Uuid);

impl InstanceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
    pub fn as_hyphenated(&self) -> String {
        self.0.as_hyphenated().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub Uuid);

impl AccountId {
    pub fn as_hyphenated(&self) -> String {
        self.0.as_hyphenated().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Vanilla,
    Fabric,
    Forge,
}

impl Loader {
    pub fn as_str(self) -> &'static str {
        match self {
            Loader::Vanilla => "vanilla",
            Loader::Fabric => "fabric",
            Loader::Forge => "forge",
        }
    }
}
```

`crates/engine/src/error.rs`:

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Microsoft CLIENT_ID is not configured")]
    AuthNotConfigured,
    #[error("account session expired")]
    AuthExpired,
    #[error("auth failed: {message}")]
    AuthFailed { message: String },
    #[error("this Microsoft account does not own Minecraft")]
    MinecraftNotOwned,
    #[error("a login is already in progress")]
    LoginInProgress,
    #[error("no Microsoft account selected")]
    NoAccount,
    #[error("Minecraft version not found: {id}")]
    VersionNotFound { id: String },
    #[error("no {loader:?} build for Minecraft {minecraft}")]
    LoaderUnavailable { loader: crate::ids::Loader, minecraft: String },
    #[error("checksum mismatch for {path:?}: expected {expected}, got {actual}")]
    ChecksumMismatch { path: PathBuf, expected: String, actual: String },
    #[error("java binary not found")]
    JavaNotFound,
    #[error("sandbox unavailable: {reason}")]
    SandboxUnavailable { reason: String },
    #[error("cancelled")]
    Cancelled,
    #[error("instance is already preparing or running")]
    InstanceBusy,
    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("crypto failure")]
    Crypto,
    #[error("http {status} for {url}")]
    Http { url: String, status: u16 },
}

impl EngineError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io { path: path.into(), source }
    }
}
```

Do **not** add `rusqlite` in this task. Use this `Sqlite` variant with a placeholder so the enum matches the spec:

```rust
    #[error("sqlite: {0}")]
    Sqlite(String),
```

Task 2 replaces `String` with `#[from] rusqlite::Error`.

`crates/engine/src/paths.rs`:

```rust
use std::path::PathBuf;
use crate::error::EngineError;

#[derive(Debug, Clone)]
pub struct LauncherPaths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub instances: PathBuf,
    pub cache_meta: PathBuf,
    pub cache_libraries: PathBuf,
    pub cache_assets_indexes: PathBuf,
    pub cache_assets_objects: PathBuf,
    pub cache_assets_virtual: PathBuf,
    pub cache_runtime: PathBuf,
    pub cache_natives: PathBuf,
}

impl LauncherPaths {
    pub fn new(root: PathBuf) -> Self {
        let cache = root.join("cache");
        let assets = cache.join("assets");
        Self {
            db: root.join("kmine.db"),
            instances: root.join("instances"),
            cache_meta: cache.join("meta"),
            cache_libraries: cache.join("libraries"),
            cache_assets_indexes: assets.join("indexes"),
            cache_assets_objects: assets.join("objects"),
            cache_assets_virtual: assets.join("virtual").join("legacy"),
            cache_runtime: cache.join("runtime"),
            cache_natives: cache.join("natives"),
            root,
        }
    }

    pub fn default_root() -> PathBuf {
        directories::BaseDirs::new()
            .map(|b| b.data_dir().join("kmine"))
            .unwrap_or_else(|| PathBuf::from("kmine-data"))
    }

    pub fn create_dirs(&self) -> Result<(), EngineError> {
        for dir in [
            &self.root,
            &self.instances,
            &self.cache_meta,
            &self.cache_libraries,
            &self.cache_assets_indexes,
            &self.cache_assets_objects,
            &self.cache_assets_virtual,
            &self.cache_runtime,
            &self.cache_natives,
        ] {
            std::fs::create_dir_all(dir).map_err(|e| EngineError::io(dir, e))?;
        }
        Ok(())
    }

    pub fn instance_dir(&self, slug: &str) -> PathBuf {
        self.instances.join(slug)
    }

    pub fn instance_minecraft(&self, slug: &str) -> PathBuf {
        self.instance_dir(slug).join(".minecraft")
    }
}
```

`default_root` must resolve to:

- macOS: `~/Library/Application Support/kmine/`
- Windows: `%APPDATA%/kmine/`
- Linux: `$XDG_DATA_HOME/kmine` or `~/.local/share/kmine`

Use `directories::BaseDirs` / `ProjectDirs` so those three match. If `ProjectDirs::from("dev", "kmine", "kmine")` yields `…/kmine/kmine`, switch to `BaseDirs::new().data_dir().join("kmine")`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS. `cargo check -p kmine` still builds the hello-world binary.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/engine src/main.rs Cargo.lock
git commit -m "feat: add kmine-engine workspace with paths and ids"
```

---

### Task 2: SQLite migrate + config kv

**Files:**
- Modify: `crates/engine/Cargo.toml` (add `rusqlite` with `bundled`)
- Modify: `crates/engine/src/error.rs` (`Sqlite(#[from] rusqlite::Error)`)
- Create: `crates/engine/src/store/mod.rs`
- Create: `crates/engine/src/store/migrate.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod store;`)
- Test: `crates/engine/src/store/mod.rs`

**Interfaces:**
- Consumes: `EngineError`, `LauncherPaths`
- Produces:
  - `pub struct Store { conn: rusqlite::Connection }`
  - `impl Store { pub fn open_file(path: &Path) -> Result<Self, EngineError>; pub fn get_config(&self, key: &str) -> Result<Option<String>, EngineError>; pub fn set_config(&self, key: &str, value: &str) -> Result<(), EngineError>; }`
  - After open: `PRAGMA journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`, `user_version >= 1`
  - Tables: `config`, `accounts`, `secrets`, `instances` exactly as in the spec

- [ ] **Step 1: Write the failing migrate/config tests**

```rust
#[cfg(test)]
mod tests {
    use super::Store;

    fn open_mem() -> Store {
        Store::open_file(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn migrate_sets_user_version_1() {
        let store = open_mem();
        let v: i32 = store.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn config_round_trip() {
        let store = open_mem();
        assert_eq!(store.get_config("selected_account").unwrap(), None);
        store.set_config("selected_account", "\"abc\"").unwrap();
        assert_eq!(store.get_config("selected_account").unwrap().as_deref(), Some("\"abc\""));
        store.set_config("window", r#"{"width":800,"height":600}"#).unwrap();
        assert!(store.get_config("window").unwrap().unwrap().contains("800"));
    }

    #[test]
    fn migrate_is_idempotent() {
        let store = open_mem();
        store.migrate().unwrap();
        store.migrate().unwrap();
        let v: i32 = store.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 1);
    }
}
```

`Store::open_file` and `migrate` do not exist yet.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kmine-engine --lib store::`

Expected: FAIL compile (`store` module missing).

- [ ] **Step 3: Implement migrate + config**

Add to `crates/engine/Cargo.toml`:

```toml
rusqlite = { version = "0.38", features = ["bundled", "blob"] }
```

`crates/engine/src/store/migrate.rs`:

```rust
use rusqlite::Connection;
use crate::error::EngineError;

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
```

`crates/engine/src/store/mod.rs`:

```rust
mod migrate;

use std::path::Path;
use rusqlite::Connection;
use crate::error::EngineError;

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
        let mut stmt = self.conn.prepare("SELECT value FROM config WHERE key = ?1")?;
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
```

Switch `EngineError::Sqlite` to `Sqlite(#[from] rusqlite::Error)`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: add kmine.db schema v1 and config kv"
```

---

### Task 3: AES-GCM crypto + memory keychain

**Files:**
- Modify: `crates/engine/Cargo.toml` (add `aes-gcm`, `rand`)
- Create: `crates/engine/src/store/crypto.rs`
- Create: `crates/engine/src/store/keychain.rs`
- Modify: `crates/engine/src/store/mod.rs` (`mod crypto; mod keychain;`)
- Test: `crates/engine/src/store/crypto.rs`, `crates/engine/src/store/keychain.rs`

**Interfaces:**
- Consumes: `EngineError::Crypto`
- Produces:
  - `pub const MASTER_KEY_LEN: usize = 32;`
  - `pub fn generate_master_key() -> [u8; 32]`
  - `pub fn seal(key: &[u8; 32], id: &str, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), EngineError>` → `(nonce /*12*/, ciphertext_with_tag)`
  - `pub fn open(key: &[u8; 32], id: &str, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, EngineError>`
  - AAD is `id.as_bytes()`
  - `pub trait Keychain: Send + Sync { fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError>; fn set_master_key(&self, key: &[u8; 32]) -> Result<(), EngineError>; }`
  - `pub struct MemoryKeychain { key: std::sync::Mutex<Option<[u8; 32]>> }`

- [ ] **Step 1: Write failing crypto tests**

```rust
#[cfg(test)]
mod tests {
    use super::{generate_master_key, open, seal};

    #[test]
    fn seal_open_round_trip() {
        let key = generate_master_key();
        let (nonce, ct) = seal(&key, "account/u1", b"hello").unwrap();
        assert_eq!(nonce.len(), 12);
        let pt = open(&key, "account/u1", &nonce, &ct).unwrap();
        assert_eq!(pt, b"hello");
    }

    #[test]
    fn wrong_aad_fails() {
        let key = generate_master_key();
        let (nonce, ct) = seal(&key, "account/u1", b"hello").unwrap();
        assert!(open(&key, "account/u2", &nonce, &ct).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let (nonce, ct) = seal(&generate_master_key(), "id", b"x").unwrap();
        assert!(open(&generate_master_key(), "id", &nonce, &ct).is_err());
    }
}
```

```rust
#[cfg(test)]
mod tests {
    use super::{Keychain, MemoryKeychain};

    #[test]
    fn memory_keychain_persists_in_process() {
        let kc = MemoryKeychain::new();
        assert!(kc.get_master_key().unwrap().is_none());
        let key = [7u8; 32];
        kc.set_master_key(&key).unwrap();
        assert_eq!(kc.get_master_key().unwrap(), Some(key));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib store::crypto`

Expected: FAIL compile.

- [ ] **Step 3: Implement**

```rust
// store/crypto.rs
use aes_gcm::{Aes256Gcm, KeyInit, aead::{Aead, Payload}};
use aes_gcm::aead::generic_array::GenericArray;
use rand::RngCore;
use crate::error::EngineError;

pub const MASTER_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

pub fn generate_master_key() -> [u8; MASTER_KEY_LEN] {
    let mut key = [0u8; MASTER_KEY_LEN];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

pub fn seal(key: &[u8; 32], id: &str, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), EngineError> {
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(
            GenericArray::from_slice(&nonce),
            Payload { msg: plaintext, aad: id.as_bytes() },
        )
        .map_err(|_| EngineError::Crypto)?;
    Ok((nonce.to_vec(), ct))
}

pub fn open(key: &[u8; 32], id: &str, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, EngineError> {
    if nonce.len() != NONCE_LEN {
        return Err(EngineError::Crypto);
    }
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    cipher
        .decrypt(
            GenericArray::from_slice(nonce),
            Payload { msg: ciphertext, aad: id.as_bytes() },
        )
        .map_err(|_| EngineError::Crypto)
}
```

```rust
// store/keychain.rs
use std::sync::Mutex;
use crate::error::EngineError;

pub trait Keychain: Send + Sync {
    fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError>;
    fn set_master_key(&self, key: &[u8; 32]) -> Result<(), EngineError>;
}

pub struct MemoryKeychain {
    key: Mutex<Option<[u8; 32]>>,
}

impl MemoryKeychain {
    pub fn new() -> Self {
        Self { key: Mutex::new(None) }
    }
}

impl Keychain for MemoryKeychain {
    fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError> {
        Ok(*self.key.lock().expect("keychain mutex"))
    }
    fn set_master_key(&self, key: &[u8; 32]) -> Result<(), EngineError> {
        *self.key.lock().expect("keychain mutex") = Some(*key);
        Ok(())
    }
}
```

OS keychain (`OsKeychain`) is **not** in this task.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: seal secrets with AES-256-GCM and memory keychain"
```

---

### Task 4: Store secrets + OsKeychain + ensure master key

**Files:**
- Modify: `crates/engine/src/store/mod.rs`
- Modify: `crates/engine/src/store/keychain.rs` (add `OsKeychain`)
- Modify: `crates/engine/Cargo.toml` (macOS `security-framework`, Windows `windows`, Linux `oo7`)
- Test: `crates/engine/src/store/mod.rs`

**Interfaces:**
- Consumes: `seal`/`open`, `Keychain`, `generate_master_key`
- Produces:
  - `impl Store { pub fn open(path: &Path, keychain: &dyn Keychain) -> Result<(Self, [u8; 32]), EngineError>; pub fn put_secret(&self, key: &[u8; 32], id: &str, plaintext: &[u8]) -> Result<(), EngineError>; pub fn get_secret(&self, key: &[u8; 32], id: &str) -> Result<Option<Vec<u8>>, EngineError>; pub fn delete_secret(&self, id: &str) -> Result<(), EngineError>; }`
  - `open`: migrate, then `get_master_key`; if `None`, `generate_master_key` + `set_master_key`. If key exists, use it even if old secrets cannot decrypt (caller surfaces `AuthExpired`).
  - `OsKeychain` service `dev.kmine.launcher`, account `master-key`.
  - `pub fn ensure_master_key(keychain: &dyn Keychain) -> Result<[u8; 32], EngineError>`

- [ ] **Step 1: Write failing store-secret tests**

```rust
#[test]
fn put_get_secret_round_trip() {
    let kc = MemoryKeychain::new();
    let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
    store.put_secret(&key, "account/u1", br#"{"msa_refresh":"r"}"#).unwrap();
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kmine-engine --lib store::tests::put_get_secret_round_trip`

Expected: FAIL compile (`Store::open` / `put_secret` missing).

- [ ] **Step 3: Implement Store secret APIs and OsKeychain**

`ensure_master_key` + `Store::open` / `put_secret` / `get_secret` / `delete_secret` as specified. `put_secret` overwrites on same `id`.

`OsKeychain` (platform modules inside `keychain.rs`):

- macOS: `security_framework::os::macos::keychain::SecKeychain::default()` + `find_generic_password` / `set_generic_password` with service `dev.kmine.launcher`, account `master-key`. Treat `errSecItemNotFound` as `Ok(None)`.
- Windows: `CredReadW` / `CredWriteW` target `dev.kmine.launcher/master-key`, type generic, persist local machine. `ERROR_NOT_FOUND` → `None`.
- Linux: `oo7` is async. Keep `Keychain` sync and call it via `pollster::block_on`. Item attributes: service `dev.kmine.launcher`, account `master-key`. Add `pollster` only under `target_os = "linux"`.
- Other OS: `get_master_key` / `set_master_key` return `EngineError::Crypto`.

Do not log the key bytes.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS (memory keychain path). OsKeychain is compiled but not required to hit a real keychain in unit tests.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: persist encrypted secrets and OS master key"
```

---

### Task 5: Accounts CRUD

**Files:**
- Modify: `crates/engine/src/store/mod.rs`
- Create: `crates/engine/src/types.rs` (move `AccountSummary` here)
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/src/store/mod.rs`

**Interfaces:**
- Consumes: `AccountId`, `Store::set_config`, secrets
- Produces:
  - `pub struct AccountRecord { pub uuid: AccountId, pub username: String, pub added_at: i64, pub last_used_at: Option<i64> }`
  - `Store::upsert_account(&self, rec: &AccountRecord) -> Result<(), EngineError>`
  - `Store::list_accounts(&self) -> Result<Vec<AccountRecord>, EngineError>` (stable order: `added_at ASC`)
  - `Store::delete_account(&self, id: AccountId) -> Result<(), EngineError>` — deletes `accounts` row **and** `secrets` row `account/<uuid>`
  - `Store::selected_account(&self) -> Result<Option<AccountId>, EngineError>` reads `config.selected_account` JSON string or null
  - `Store::set_selected_account(&self, id: Option<AccountId>) -> Result<(), EngineError>`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn upsert_list_select_delete_account() {
    let kc = MemoryKeychain::new();
    let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
    let id = AccountId(uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap());
    store.upsert_account(&AccountRecord {
        uuid: id,
        username: "Steve".into(),
        added_at: 10,
        last_used_at: None,
    }).unwrap();
    store.put_secret(&key, &format!("account/{}", id.as_hyphenated()), b"{}").unwrap();
    store.set_selected_account(Some(id)).unwrap();
    assert_eq!(store.list_accounts().unwrap().len(), 1);
    assert_eq!(store.selected_account().unwrap(), Some(id));
    store.delete_account(id).unwrap();
    assert!(store.list_accounts().unwrap().is_empty());
    assert!(store.get_secret(&key, &format!("account/{}", id.as_hyphenated())).unwrap().is_none());
    assert_eq!(store.selected_account().unwrap(), Some(id)); // config left as-is; Engine clears it in Task 7
}
```

Add a second test `delete_account_sets_nothing_if_missing` that `delete_account` on unknown uuid is `Ok(())`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kmine-engine --lib store::tests::upsert_list_select_delete_account`

Expected: FAIL compile.

- [ ] **Step 3: Implement account methods**

SQL:

```sql
INSERT INTO accounts(uuid, username, added_at, last_used_at)
VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(uuid) DO UPDATE SET username = excluded.username, last_used_at = excluded.last_used_at;
```

`set_selected_account(None)` writes JSON `null`. `set_selected_account(Some(id))` writes `"<hyphenated>"` (JSON string).

`delete_account` runs in one transaction: `DELETE FROM secrets WHERE id = ?` then `DELETE FROM accounts WHERE uuid = ?`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: store Microsoft accounts and selected_account"
```

---

### Task 6: Instance rows, slugs, folders

**Files:**
- Create: `crates/engine/src/instance/mod.rs`
- Modify: `crates/engine/src/store/mod.rs`
- Modify: `crates/engine/src/types.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/src/instance/mod.rs`, `crates/engine/src/store/mod.rs`

**Interfaces:**
- Consumes: `LauncherPaths`, `InstanceId`, `Loader`, `Store`
- Produces:
  - `pub fn slug_from_name(name: &str) -> String` — trim; strip `/ \ : * ? " < > |`; empty → `"instance"`
  - `pub fn unique_slug(desired: &str, taken: &[String]) -> String` — `name`, then `name (2)` … `name (999)`, then `name (<uuid>)`
  - `pub struct InstanceRow` — all `instances` columns as typed fields (`loader: Loader`, `sandbox: bool`, `icon_png: Option<Vec<u8>>`, …)
  - `Store::insert_instance(&self, row: &InstanceRow)`
  - `Store::get_instance(&self, id: InstanceId) -> Result<Option<InstanceRow>, EngineError>`
  - `Store::list_instances(&self) -> Result<Vec<InstanceRow>, EngineError>` order `last_played_at DESC NULLS LAST, name COLLATE NOCASE`
  - `Store::update_instance(&self, row: &InstanceRow)`
  - `Store::delete_instance(&self, id: InstanceId)`
  - `Store::list_slugs(&self) -> Result<Vec<String>, EngineError>`
  - `pub fn create_instance_dirs(paths: &LauncherPaths, slug: &str) -> Result<(), EngineError>` creates `instances/<slug>/.minecraft`
  - `pub fn rename_instance_dir(paths: &LauncherPaths, old: &str, new: &str) -> Result<(), EngineError>`
  - `pub fn delete_instance_dir(paths: &LauncherPaths, slug: &str) -> Result<(), EngineError>` — `remove_dir_all`; `NotFound` is Ok

- [ ] **Step 1: Write failing slug tests**

```rust
#[test]
fn slug_strips_forbidden_chars() {
    assert_eq!(slug_from_name("  My/Pack?  "), "MyPack");
    assert_eq!(slug_from_name("   "), "instance");
}

#[test]
fn unique_slug_adds_numeric_suffix() {
    let taken = vec!["Foo".into(), "Foo (2)".into()];
    assert_eq!(unique_slug("Foo", &taken), "Foo (3)");
    assert_eq!(unique_slug("Bar", &taken), "Bar");
}
```

Instance store test using tempfile + MemoryKeychain:

```rust
#[test]
fn insert_and_list_instance() {
    let kc = MemoryKeychain::new();
    let (store, _) = Store::open(Path::new(":memory:"), &kc).unwrap();
    let id = InstanceId::new();
    store.insert_instance(&InstanceRow {
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
    }).unwrap();
    let list = store.list_instances().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].minecraft_version, "1.21.1");
}
```

Folder test:

```rust
#[test]
fn create_and_rename_instance_dirs() {
    let root = tempfile::tempdir().unwrap();
    let paths = LauncherPaths::new(root.path().to_path_buf());
    paths.create_dirs().unwrap();
    create_instance_dirs(&paths, "Alpha").unwrap();
    assert!(paths.instance_minecraft("Alpha").is_dir());
    rename_instance_dir(&paths, "Alpha", "Beta").unwrap();
    assert!(!paths.instance_dir("Alpha").exists());
    assert!(paths.instance_minecraft("Beta").is_dir());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib instance::`

Expected: FAIL compile.

- [ ] **Step 3: Implement slug helpers, InstanceRow, store SQL, directory helpers**

`insert_instance` uses a single `INSERT`. `update_instance` updates every mutable column by `id`. Store `loader` with `Loader::as_str()`. Parse with:

```rust
fn loader_from_db(s: &str) -> Result<Loader, rusqlite::Error> {
    match s {
        "vanilla" => Ok(Loader::Vanilla),
        "fabric" => Ok(Loader::Fabric),
        "forge" => Ok(Loader::Forge),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, s)),
        )),
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: persist instances and manage instance folders"
```

---

### Task 7: Engine facade, handle, events, instance/account methods

**Files:**
- Create: `crates/engine/src/types.rs` fields for `CreateInstance`, `InstancePatch`, `InstanceSummary`, `AccountSummary`, `ProgressSink`, `SandboxStatus` (if not already all there)
- Modify: `crates/engine/src/lib.rs` — `Engine`, `EngineHandle`, `Event`
- Modify: `crates/engine/Cargo.toml` (`tokio`, `tokio-util`, `parking_lot`)
- Test: `crates/engine/src/lib.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: `Store`, `LauncherPaths`, `OsKeychain`/`MemoryKeychain`, instance dirs
- Produces exactly the spec methods that do not yet launch:

```rust
pub struct Engine {
    paths: LauncherPaths,
    store: parking_lot::Mutex<Store>,
    master_key: [u8; 32],
    events: tokio::sync::broadcast::Sender<Event>,
    processes: parking_lot::Mutex<std::collections::HashMap<InstanceId, Running>>,
    login_lock: tokio::sync::Mutex<bool>,
}

pub struct Running; // filled in Task 13

pub enum Event {
    InstancesChanged,
    AccountsChanged,
    Progress { id: InstanceId, title: String, done: u64, total: u64 },
    PrepareFinished { id: InstanceId, ok: bool },
    LogLine { instance_id: InstanceId, stream: LogStream, text: String },
    ProcessExited { instance_id: InstanceId, code: Option<i32> },
    AuthRequired,
    Error(String),
}

pub enum LogStream { Stdout, Stderr }

impl Engine {
    pub async fn open(paths: LauncherPaths) -> Result<Self, EngineError>; // uses OsKeychain
    #[cfg(test)]
    pub fn open_with_keychain(paths: LauncherPaths, kc: &dyn Keychain) -> Result<Self, EngineError>;

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event>;

    pub fn list_accounts(&self) -> Result<Vec<AccountSummary>, EngineError>;
    pub fn list_instances(&self) -> Result<Vec<InstanceSummary>, EngineError>;
    pub fn sandbox_status(&self) -> SandboxStatus; // for now always Available on macos; Linux checks bwrap later

    pub async fn create_instance(&self, spec: CreateInstance) -> Result<InstanceId, EngineError>;
    pub async fn rename_instance(&self, id: InstanceId, name: String) -> Result<(), EngineError>;
    pub async fn delete_instance(&self, id: InstanceId) -> Result<(), EngineError>;
    pub async fn update_instance(&self, id: InstanceId, patch: InstancePatch) -> Result<(), EngineError>;

    pub async fn select_account(&self, id: AccountId) -> Result<(), EngineError>;
    pub async fn delete_account(&self, id: AccountId) -> Result<(), EngineError>;
}

pub struct EngineHandle {
    engine: std::sync::Arc<Engine>,
    rt: tokio::runtime::Handle,
}
```

`create_instance` algorithm:

1. `slug = unique_slug(slug_from_name(&spec.name), &store.list_slugs()?)`
2. `create_instance_dirs`
3. `insert_instance` with `InstanceId::new()`, `created_at = now_ms()`
4. if insert fails, `delete_instance_dir` then return err
5. emit `InstancesChanged`

`rename_instance`:

1. load row
2. `new_slug = unique_slug(slug_from_name(&name), others)`
3. if slug changed: `rename_instance_dir` **first**; on err return `EngineError::io` and do not write DB
4. update name+slug; emit `InstancesChanged`

`delete_instance`:

1. `delete_instance_dir`
2. `store.delete_instance`
3. emit `InstancesChanged`

`delete_account`:

1. `store.delete_account`
2. if `selected_account == id`, `set_selected_account(None)`
3. emit `AccountsChanged`

`InstancePatch` apply: only assign fields that are `Some(...)`. `Some(None)` sets SQL NULL.

`list_instances` sets `running` from `processes` map (empty until Task 13).

- [ ] **Step 1: Write failing engine tests**

```rust
#[tokio::test]
async fn create_rename_delete_instance() {
    let root = tempfile::tempdir().unwrap();
    let paths = LauncherPaths::new(root.path().to_path_buf());
    paths.create_dirs().unwrap();
    let kc = MemoryKeychain::new();
    let engine = Engine::open_with_keychain(paths.clone(), &kc).unwrap();
    let id = engine.create_instance(CreateInstance {
        name: "One".into(),
        minecraft_version: "1.21.1".into(),
        loader: Loader::Vanilla,
        loader_version: None,
        icon_png: None,
    }).await.unwrap();
    assert_eq!(engine.list_instances().unwrap()[0].name, "One");
    assert!(paths.instance_minecraft("One").is_dir());
    engine.rename_instance(id, "Two".into()).await.unwrap();
    assert!(paths.instance_minecraft("Two").is_dir());
    engine.delete_instance(id).await.unwrap();
    assert!(engine.list_instances().unwrap().is_empty());
}
```

Need `tokio` with `macros`, `rt-multi-thread`, `sync`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kmine-engine --lib create_rename_delete_instance`

Expected: FAIL compile.

- [ ] **Step 3: Implement `Engine` / events / the methods above**

`Engine::open` calls `paths.create_dirs()`, `Store::open(&paths.db, &OsKeychain)`, stores the master key.

`start_login`, `prepare`, `spawn`, `kill`, content, quick play are **not** implemented. Give them later. Do not add stub `todo!()` in public API that tests call. Only implement what this task tests plus `list_*` / `select_account` / `update_instance`.

Add a test `update_instance_clears_java_path` that passes `InstancePatch { java_path: Some(None), ..all None }` and asserts the row's `java_path` is `None`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: add Engine facade for instances and accounts"
```

---

### Task 8: GPUI shell — sidebar + create modal

**Files:**
- Modify: `src/main.rs`
- Create: `src/app.rs`
- Create: `src/screens/mod.rs`
- Create: `src/screens/instances.rs`
- Create: `src/screens/instance_play.rs` (placeholder Play tab: name + disabled Play)
- Create: `src/modals/mod.rs`
- Create: `src/modals/create_instance.rs`

**Interfaces:**
- Consumes: `EngineHandle`, `CreateInstance`, `InstanceSummary`, `Loader`
- Produces: a window that lists instances and can create one

There is no GPUI unit test. Verification is `cargo run`.

- [ ] **Step 1: Write a compile-only smoke by replacing hello world**

No failing unit test. Start the runtime in `main`:

```rust
use std::sync::Arc;
use gpui::*;
use gpui_component::Root;
use kmine_engine::{Engine, LauncherPaths};

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio");
    let paths = LauncherPaths::new(LauncherPaths::default_root());
    let engine = runtime
        .block_on(Engine::open(paths))
        .expect("engine");
    let engine = Arc::new(engine);

    let app = Application::new();
    app.run(move |cx| {
        gpui_component::init(cx);
        cx.activate(true);
        let engine = engine.clone();
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|cx| crate::app::KmineApp::new(engine, cx));
                cx.new(|cx| Root::new(view.into(), window, cx))
            })
            .expect("window");
        })
        .detach();
    });
}
```

Bootstrap must call `gpui_component::init(cx)` before opening a window. Prefer `Application::new().run`. If this gpui revision has no `Application::new`, use `gpui_platform::application().run` with the same closure.

- [ ] **Step 2: `cargo run` once to see the compile errors**

Run: `cargo run`

Expected: FAIL compile (`KmineApp` missing).

- [ ] **Step 3: Implement `KmineApp` layout**

`KmineApp` fields: `engine: Arc<Engine>`, `instances: Vec<InstanceSummary>`, `selected: Option<InstanceId>`, `show_create: bool`, `status: String`.

On `new`: `instances = engine.list_instances().unwrap_or_default()`.

Layout (gpui-component `h_flex` / `v_flex` / `Button` / `List`):

```
left 240px: "kmine" label, list of instance names, "+ Create" button
right: if selected, `instance_play::PlayTab`, else empty state "Select an instance"
```

Create modal (`show_create`): fields name (default `"New Instance"`), version text (default `"1.21.1"`), loader dropdown Vanilla/Fabric/Forge. Submit calls `runtime.block_on` is **forbidden on the UI thread**. Use `cx.spawn` + `tokio::task::spawn_blocking` **or** keep `runtime.handle()` on `KmineApp` and `handle.spawn(async { engine.create_instance(...) })`, then `cx.update` to refresh `list_instances`.

After create: close modal, select new id, refresh list.

`PlayTab` this task: show `name`, `minecraft_version`, `loader`; Play button rendered but `disabled(true)`.

- [ ] **Step 4: Verify**

Run: `cargo run`

Expected: window opens, Create makes a row, folder appears under the data dir `instances/<slug>/.minecraft`.

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src Cargo.toml Cargo.lock
git commit -m "feat: show instance list and create modal in GPUI"
```

---

### Task 9: Mojang version JSON, rules, argv (no download)

**Files:**
- Create: `crates/engine/src/mojang/mod.rs`
- Create: `crates/engine/src/mojang/rules.rs`
- Create: `crates/engine/src/mojang/args.rs`
- Create: `crates/engine/tests/fixtures/version_1_21.json`
- Create: `crates/engine/tests/fixtures/version_1_12.json`
- Create: `crates/engine/src/types.rs` (`LaunchPlan`, `SandboxSpec` if missing)
- Test: `crates/engine/src/mojang/args.rs`, `crates/engine/src/mojang/rules.rs`

**Interfaces:**
- Consumes: none of HTTP
- Produces:
  - `pub struct VersionInfo` — deserialized Mojang version JSON (`id`, `mainClass`, `arguments`, `minecraftArguments`, `libraries`, `assetIndex`, `downloads`, `javaVersion`, `logging`)
  - `pub struct Rule { action, os: Option<RuleOs>, features: Option<RuleFeatures> }`
  - `pub fn current_os_name() -> &'static str` — `osx` / `linux` / `windows`
  - `pub fn current_os_arch() -> &'static str` — `x86` / `x86_64` / `arm64`
  - `pub fn rule_allows(rules: &[Rule], features: &FeatureSet) -> bool`
  - `pub struct FeatureSet { pub demo: bool, pub custom_resolution: bool, pub quick_play_single: bool, pub quick_play_multi: bool }`
  - `pub struct ArgContext { pub auth_player_name, auth_uuid, auth_access_token, user_type, version_name, game_directory, assets_root, assets_index_name, natives_directory, launcher_name, launcher_version, classpath, resolution_width, resolution_height, quick_play_singleplayer: Option<String>, quick_play_multiplayer: Option<String> }`
  - `pub fn interpolate(arg: &str, ctx: &ArgContext) -> String`
  - `pub fn build_args(version: &VersionInfo, ctx: &ArgContext, features: &FeatureSet) -> (Vec<String>, Vec<String>)` → `(jvm_args, game_args)`
  - Legacy: split `minecraftArguments` on whitespace, interpolate each token; jvm args default to `["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"]`

Rule algorithm (Mojang):

- No rules → allow.
- Otherwise start disallowed. Each matching rule sets allow/disallow.
- A rule matches if OS name/arch match (absent field = any) and every `features` key requested by the rule is true in `FeatureSet`. Features the rule does not mention are ignored.
- Drop a ruled argument when the rule set disallows it. Do not emit feature-gated quick-play args unless that feature is true.

`interpolate`: replace the spec placeholders. Unknown `${...}` stays unchanged.

Classpath string: join `LaunchPlan.classpath` with `:` on Unix and `;` on Windows.

- [ ] **Step 1: Add fixtures and failing tests**

`tests/fixtures/version_1_21.json` (minimal):

```json
{
  "id": "1.21.1",
  "mainClass": "net.minecraft.client.main.Main",
  "arguments": {
    "game": [
      "--username", "${auth_player_name}",
      "--version", "${version_name}",
      "--gameDir", "${game_directory}",
      "--assetsDir", "${assets_root}",
      "--assetIndex", "${assets_index_name}",
      "--uuid", "${auth_uuid}",
      "--accessToken", "${auth_access_token}",
      "--userType", "${user_type}",
      { "rules": [{ "action": "allow", "features": { "is_demo_user": true } }], "value": "--demo" },
      { "rules": [{ "action": "allow", "features": { "is_quick_play_singleplayer": true } }], "value": ["--quickPlaySingleplayer", "${quickPlaySingleplayer}"] }
    ],
    "jvm": [
      { "rules": [{ "action": "allow", "os": { "name": "osx" } }], "value": ["-XstartOnFirstThread"] },
      "-Djava.library.path=${natives_directory}",
      "-cp",
      "${classpath}"
    ]
  },
  "assetIndex": { "id": "17", "sha1": "abc", "size": 1, "totalSize": 1, "url": "https://example.invalid/17.json" },
  "assets": "17",
  "downloads": { "client": { "sha1": "aa", "size": 1, "url": "https://example.invalid/client.jar" } },
  "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
  "libraries": []
}
```

`tests/fixtures/version_1_12.json`:

```json
{
  "id": "1.12.2",
  "mainClass": "net.minecraft.client.main.Main",
  "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory} --assetsDir ${assets_root} --assetIndex ${assets_index_name} --uuid ${auth_uuid} --accessToken ${auth_access_token} --userType ${user_type}",
  "assetIndex": { "id": "1.12", "sha1": "abc", "size": 1, "totalSize": 1, "url": "https://example.invalid/1.12.json" },
  "assets": "1.12",
  "downloads": { "client": { "sha1": "bb", "size": 1, "url": "https://example.invalid/client.jar" } },
  "libraries": []
}
```

Tests:

```rust
#[test]
fn demo_arg_only_when_feature_set() {
    let v = load_fixture("version_1_21.json");
    let ctx = sample_ctx();
    let mut feat = FeatureSet::default();
    let (_, game) = build_args(&v, &ctx, &feat);
    assert!(!game.iter().any(|a| a == "--demo"));
    feat.demo = true;
    let (_, game) = build_args(&v, &ctx, &feat);
    assert!(game.iter().any(|a| a == "--demo"));
}

#[test]
fn interpolates_player_name() {
    let v = load_fixture("version_1_12.json");
    let ctx = sample_ctx();
    let (_, game) = build_args(&v, &ctx, &FeatureSet::default());
    assert!(game.windows(2).any(|w| w[0] == "--username" && w[1] == "Steve"));
}

#[test]
fn osx_jvm_flag_only_on_macos() {
    let v = load_fixture("version_1_21.json");
    let (jvm, _) = build_args(&v, &sample_ctx(), &FeatureSet::default());
    let has = jvm.iter().any(|a| a == "-XstartOnFirstThread");
    assert_eq!(has, cfg!(target_os = "macos"));
}
```

`load_fixture` reads `CARGO_MANIFEST_DIR/tests/fixtures/<name>`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib mojang::`

Expected: FAIL compile.

- [ ] **Step 3: Implement VersionInfo serde (untagged LaunchArgument: string | {rules, value: string|array}), rules, interpolate, build_args**

`LaunchArgument` enum:

```rust
pub enum LaunchArgument {
    Value(String),
    Ruled { rules: Vec<Rule>, value: ArgValue },
}
pub enum ArgValue { One(String), Many(Vec<String>) }
```

Use `#[serde(untagged)]`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: parse Mojang version JSON and build launch args"
```

---

### Task 10: HTTP download + sha1 cache

**Files:**
- Modify: `crates/engine/Cargo.toml` (`reqwest` rustls + json, `sha1`, `hex`, `tokio-util`)
- Create: `crates/engine/src/http.rs`
- Test: `crates/engine/src/http.rs` with `wiremock`

**Interfaces:**
- Consumes: `EngineError::ChecksumMismatch`, `Cancelled`, `Http`, `Io`
- Produces:
  - `pub struct HttpFiles { pub client: reqwest::Client }`
  - `impl HttpFiles { pub fn new() -> Result<Self, EngineError>; pub async fn get_json<T: DeserializeOwned>(&self, url: &str, cancel: &CancellationToken) -> Result<T, EngineError>; pub async fn download_sha1(&self, url: &str, dest: &Path, expected_sha1: Option<&str>, cancel: &CancellationToken) -> Result<(), EngineError>; }`
  - Cache hit: dest exists AND (expected sha1 matches OR (no sha1 and dest len > 0)).
  - Mismatch: delete dest, download, hash while writing to `dest.with_extension("part")`, `rename`.
  - Empty file is never a hit.
  - Cancel → `EngineError::Cancelled`.
  - Non-success HTTP → `EngineError::Http { url, status }`.

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test]
async fn downloads_and_verifies_sha1() {
    let server = wiremock::MockServer::start().await;
    let body = b"abc";
    let hash = sha1_hex(body);
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_raw(body.as_slice(), "text/plain"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("f.bin");
    let http = HttpFiles::new().unwrap();
    http.download_sha1(&format!("{}/f", server.uri()), &dest, Some(&hash), &CancellationToken::new()).await.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), body);
}

#[tokio::test]
async fn cache_hit_skips_network() {
    // write dest with correct sha1, mount a mock that panics if called (expect 0 requests)
}

#[tokio::test]
async fn bad_sha1_errors() {
    // server returns "abc", expect ChecksumMismatch
}

#[tokio::test]
async fn cancel_stops_download() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let err = HttpFiles::new().unwrap()
        .download_sha1("http://127.0.0.1:1/", Path::new("/tmp/x"), None, &cancel).await
        .unwrap_err();
    assert!(matches!(err, EngineError::Cancelled));
}
```

Add `wiremock` + `sha1` to `[dev-dependencies]` / `[dependencies]`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib http::`

Expected: FAIL compile.

- [ ] **Step 3: Implement `HttpFiles`**

Use `tokio::select!` on `cancel.cancelled()` vs the reqwest future. Compute sha1 with `sha1::Sha1` while writing. Compare lowercase hex.

User-Agent: `kmine/<CARGO_PKG_VERSION>`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: download files with sha1 cache and cancel"
```

---

### Task 11: Libraries, natives, assets, client jar

**Files:**
- Modify: `crates/engine/src/mojang/mod.rs`
- Create: `crates/engine/src/mojang/libraries.rs`
- Create: `crates/engine/src/mojang/assets.rs`
- Create: `crates/engine/tests/fixtures/assets_index.json`
- Test: `crates/engine/src/mojang/libraries.rs`, `assets.rs`

**Interfaces:**
- Consumes: `VersionInfo`, `HttpFiles`, `LauncherPaths`, `FeatureSet`
- Produces:
  - `pub struct LibraryArtifact { pub path: String, pub url: String, pub sha1: Option<String>, pub size: Option<u64>, pub extract_natives: bool }`
  - `pub fn select_libraries(version: &VersionInfo) -> Vec<LibraryArtifact>` — skip disallowed rules; include `downloads.artifact`; for legacy `natives` map, include the classifier for current OS (`natives-osx` / `natives-linux` / `natives-windows`) and set `extract_natives = true`
  - `pub async fn fetch_libraries(http, paths, artifacts, progress, cancel) -> Result<Vec<PathBuf>, EngineError>` dest `cache/libraries/<path>`
  - `pub fn natives_dir_name(artifacts: &[LibraryArtifact], sandbox: bool) -> String` — sha1 hex of sorted artifact paths + `"-sandbox"` suffix when `sandbox`
  - `pub fn extract_natives(jar: &Path, dest: &Path, exclude: &[String]) -> Result<(), EngineError>` using `zip`; skip `META-INF/` and exclude prefixes
  - `pub async fn fetch_assets(http, paths, index_url, index_sha1, index_id, game_dir, progress, cancel) -> Result<AssetsRoot, EngineError>`
  - `pub enum AssetsRoot { Objects { dir: PathBuf, index: String }, Virtual(PathBuf), Resources(PathBuf) }`
  - Object dest: `cache/assets/objects/<first two hex chars>/<sha1>`
  - If index `map_to_resources == true` → copy/link into `game_dir/resources/<object name>`
  - If index `virtual == true` → materialize under `cache/assets/virtual/legacy/<object name>`
  - `pub async fn fetch_client(http, paths, version, cancel) -> Result<PathBuf, EngineError>` dest `cache/libraries/net/minecraft/<id>/minecraft-client-<id>.jar` (or `cache/meta/<id>/client.jar` — pick **`cache/libraries/com/mojang/minecraft/<id>/minecraft-<id>-client.jar`** and use it consistently in classpath)

- [ ] **Step 1: Write failing tests**

Fixture library entry in a small `version_libs.json` with one allowed artifact and one `disallow` linux-only library. Test `select_libraries` length.

Natives test: build a zip in tempfile with `a.so` and `META-INF/MANIFEST.MF`, `extract_natives`, assert `a.so` exists and `META-INF` does not.

Assets test: wiremock index + one object; `fetch_assets`; assert object path layout `objects/ab/<sha1>`.

`natives_dir_name` test: same artifacts → same name; `sandbox=true` adds `-sandbox`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib mojang::`

Expected: FAIL compile.

- [ ] **Step 3: Implement selection, fetch, extract**

Add `zip` crate. Progress: `progress.set("Libraries", done, total)` after each file.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: fetch Minecraft libraries, natives, and assets"
```

---

### Task 12: Java resolve + Mojang runtime install

**Files:**
- Create: `crates/engine/src/java/mod.rs`
- Create: `crates/engine/src/java/platform.rs`
- Test: `crates/engine/src/java/mod.rs`

**Interfaces:**
- Consumes: `HttpFiles`, `LauncherPaths`, `VersionInfo.javaVersion`
- Produces:
  - `pub fn platform_id(os: &str, arch: &str) -> String` mapping:
    - linux + x86_64 → `linux`
    - linux + x86 → `linux-i386`
    - macos + x86_64 → `mac-os`
    - macos + aarch64 → `mac-os-arm64`
    - windows + x86_64 → `windows-x64`
    - windows + aarch64 → `windows-arm64`
    - windows + x86 → `windows-x86`
    - else `{os}-{arch}`
  - `pub fn find_java_binary(hint: &Path) -> Option<PathBuf>` — if `hint` is file named `java`/`java.exe`, use it; else try `hint/bin/java`, `hint/bin/java.exe`, `hint/Contents/Home/bin/java`
  - `pub async fn resolve_java(http, paths, version: &VersionInfo, custom: Option<&Path>, progress, cancel) -> Result<PathBuf, EngineError>`
    - custom Some → `find_java_binary` or `JavaNotFound`
    - else fetch Mojang `https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json` (cache as `cache/meta/java-all.json`)
    - component = `version.javaVersion.component` or `"jre-legacy"`
    - platform = `platform_id(std::env::consts::OS, std::env::consts::ARCH)`
    - if component missing on `mac-os-arm64`, retry `mac-os`
    - download file manifest; install files into `cache/runtime/<component>/<platform>/` (type file / directory / link). `executable: true` → `chmod 0o755` on Unix.
    - locate binary: mac `Contents/Home/bin/java`, else `bin/java` or `bin/java.exe`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn platform_id_macos_arm() {
    assert_eq!(platform_id("macos", "aarch64"), "mac-os-arm64");
    assert_eq!(platform_id("linux", "x86_64"), "linux");
}

#[test]
fn find_java_binary_accepts_bin_java() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let java = bin.join("java");
    std::fs::write(&java, b"x").unwrap();
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap(); }
    assert_eq!(find_java_binary(dir.path()).as_deref(), Some(java.as_path()));
}

#[tokio::test]
async fn resolve_custom_missing_errors() {
    let err = resolve_java(..., Some(Path::new("/no/java/here")), ...).await.unwrap_err();
    assert!(matches!(err, EngineError::JavaNotFound));
}
```

A wiremock test for runtime install: serve a tiny `all.json` + file manifest with one file `bin/java`; assert dest exists.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib java::`

Expected: FAIL compile.

- [ ] **Step 3: Implement platform map, finder, installer**

Do not copy Pandora file names. Write the installer from the Mojang file-manifest schema (`type: file|directory|link`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: resolve custom Java or install Mojang runtime"
```

---

### Task 13: Microsoft auth (refresh + login) with mocked HTTP

**Files:**
- Create: `crates/engine/src/auth/constants.rs`
- Create: `crates/engine/src/auth/mod.rs`
- Create: `crates/engine/src/auth/tokens.rs`
- Create: `crates/engine/src/auth/oauth.rs`
- Modify: `crates/engine/src/lib.rs` (`start_login`)
- Modify: `crates/engine/Cargo.toml` (`oauth2`, `chrono`, `open`, `httparse`)
- Test: `crates/engine/src/auth/mod.rs`

**Interfaces:**
- Consumes: `Store` secrets JSON, `AccountId`, `HttpFiles`
- Produces:
  - `pub const CLIENT_ID: &str = "";`
  - `pub const AUTH_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";`
  - `pub const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";`
  - `pub const REDIRECT_URL: &str = "http://127.0.0.1:47821/auth";`
  - `pub const BIND: &str = "127.0.0.1:47821";`
  - `pub struct AccountSecrets { msa_refresh, msa_access, xbl, xsts, mc_access }` with `Token { token: String, expiry: DateTime<Utc> }` and `Xsts { token, expiry, userhash }`
  - `pub fn secret_id(uuid: AccountId) -> String` → `account/<hyphenated>`
  - `pub async fn ensure_mc_token(http, store, key, account, now_now: DateTime<Utc>) -> Result<String, EngineError>`
    - if `mc_access` valid for >60s, return it
    - else walk xsts → xbl → msa_access → msa_refresh
    - `invalid_grant` → `delete_secret` + `AuthExpired`
  - `pub async fn login_with_code(http, store, key, code, pkce_verifier) -> Result<AccountSummary, EngineError>`
  - `Engine::start_login`: if `CLIENT_ID.is_empty()` → `AuthNotConfigured`. Try `login_lock.try_lock` else `LoginInProgress`. Bind `BIND`, PKCE, open browser (`open::that`), wait for `GET /auth?code=&state=`, validate state, exchange, Xbox, XSTS, login_with_xbox, GET profile. 404 profile → `MinecraftNotOwned`. Upsert account + seal secrets + `set_selected_account` + `AccountsChanged`.

Xbox bodies (public protocol):

```json
{ "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": "d=<msa>" },
  "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT" }
```

```json
{ "Properties": { "SandboxId": "RETAIL", "UserTokens": ["<xbl>"] },
  "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT" }
```

`login_with_xbox`: `{ "identityToken": "XBL3.0 x=<userhash>;<xsts>" }`.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn empty_client_id_is_not_configured() {
    assert!(CLIENT_ID.is_empty());
}

#[tokio::test]
async fn ensure_mc_token_uses_cached_token() {
    // put secrets with mc_access expiry = now + 1h
    let tok = ensure_mc_token(...).await.unwrap();
    assert_eq!(tok, "cached");
}

#[tokio::test]
async fn invalid_grant_is_auth_expired() {
    // secrets only have msa_refresh; token endpoint returns error invalid_grant
    let err = ensure_mc_token(...).await.unwrap_err();
    assert!(matches!(err, EngineError::AuthExpired));
}

#[tokio::test]
async fn start_login_without_client_id() {
    let err = engine.start_login().await.unwrap_err();
    assert!(matches!(err, EngineError::AuthNotConfigured));
}
```

Wiremock the token URL by injecting `AuthEndpoints { token_url, xbox_url, xsts_url, mc_login_url, profile_url }` on `ensure_mc_token` so tests do not hit the real network. Production passes the constants.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib auth::`

Expected: FAIL compile.

- [ ] **Step 3: Implement tokens, refresh, oauth listener, `start_login`**

Listener: `TcpListener::bind(BIND)`, read HTTP request with `httparse`, parse query, respond `200` HTML `"You can close this tab."`, shutdown.

Never log token values.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: Microsoft OAuth and Minecraft token refresh"
```

---

### Task 14: Accounts modal in GPUI

**Files:**
- Create: `src/modals/accounts.rs`
- Modify: `src/app.rs` (account button in footer, open modal)
- Modify: `src/modals/mod.rs`

**Interfaces:**
- Consumes: `Engine::list_accounts`, `start_login`, `select_account`, `delete_account`
- Produces: footer shows selected username or `"Not signed in"`; modal lists accounts

- [ ] **Step 1: Add the footer button and modal types (compile)**

`AccountsModal` state: `accounts: Vec<AccountSummary>`, `error: Option<String>`, `busy: bool`.

- [ ] **Step 2: `cargo run` to see missing module errors**

Expected: FAIL compile until files exist, then window runs.

- [ ] **Step 3: Implement UI**

- Row click → `select_account`
- Delete → `delete_account`
- "Add account" → `busy=true`, spawn `start_login`. On `AuthNotConfigured` show: `Set CLIENT_ID in crates/engine/src/auth/constants.rs and register redirect http://127.0.0.1:47821/auth`.
- `LoginInProgress` → status text
- Refresh list on `AccountsChanged`

- [ ] **Step 4: Verify**

Run: `cargo test -p kmine-engine && cargo run`

Expected: tests PASS; modal opens; Add account with empty CLIENT_ID shows the setup message.

- [ ] **Step 5: Commit**

```bash
git add src
git commit -m "feat: add accounts modal and footer identity"
```

---

### Task 15: prepare + spawn vanilla + redact + game output

**Files:**
- Create: `crates/engine/src/launch/mod.rs`
- Create: `crates/engine/src/redact.rs`
- Create: `src/game_output.rs`
- Create: `src/modals/progress.rs`
- Modify: `crates/engine/src/lib.rs` (`prepare`, `spawn`, `kill`)
- Modify: `src/screens/instance_play.rs`
- Modify: `src/app.rs`
- Test: `crates/engine/src/redact.rs`, `crates/engine/src/launch/mod.rs`

**Interfaces:**
- Consumes: instance row, `ensure_mc_token`, mojang fetch, java, libraries, assets, `build_args`
- Produces:
  - `pub fn redact_line(line: &str) -> String`
  - `Engine::prepare(...)` vanilla path (Fabric/Forge still `LoaderUnavailable` if not vanilla — **no**, Fabric/Forge are later; for this task `prepare` implements vanilla and returns `LoaderUnavailable` for other loaders)
  - `Engine::spawn` / `kill`
  - `ProgressSink` implementation in UI that sends `Event::Progress`

`redact_line` replacements (in order):

1. If a Minecraft/MSA token was used for this process, replace exact token strings with `[redacted]`.
2. Regex `(?i)accessToken=[^\\s]+` → `accessToken=[redacted]`
3. Regex `eyJ[A-Za-z0-9_-]{20,}\\.[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]+` → `[redacted]`

`prepare` vanilla (cancel between each step):

1. Load instance; if `processes` already has id or a `preparing` set contains id → `InstanceBusy`
2. Resolve account (`row.account_uuid` else selected); none → emit `AuthRequired` and return `NoAccount`
3. `ensure_mc_token`
4. GET `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` into `cache/meta/version_manifest_v2.json`
5. Find version id; missing → `VersionNotFound`
6. Download version JSON by sha1 into `cache/meta/<id>.json`
7. `resolve_java`
8. `fetch_client`, `fetch_libraries`, `extract_natives`, `fetch_assets`, download logging xml if present into `cache/meta/logconfigs/`
9. Build `ArgContext` (`launcher_name=kmine`, `user_type=msa`)
10. Apply `-Xms`/`-Xmx` from row when `Some`; append `shell_words::split(jvm_flags)`
11. `SandboxSpec { enabled: row.sandbox, allow_read: [java_home, cache_libraries, cache_assets_*, cache_runtime, java], allow_write: [cwd, natives_dir], network: true }`
12. Return `LaunchPlan`

`spawn`:

- If `plan.sandbox.enabled` → Task 17; **this task** if enabled call `EngineError::SandboxUnavailable { reason: "sandbox not implemented".into() }` so we do not silently ignore the flag.
- Else `Command::new(&plan.java)` args = `jvm_args + [main_class] + game_args`, `current_dir(cwd)`, piped stdio, extra `env`.
- Insert into `processes` with `started_at = Instant::now()`.
- Spawn a tokio task reading lines; `redact_line` then `Event::LogLine`.
- On exit: add elapsed secs to `playtime_secs`, `session_count += 1`, `last_played_at = now`, remove process, `ProcessExited` + `InstancesChanged`.

`kill`: `child.start_kill()` or `kill()`. Missing process is Ok.

Play button: if running → `kill`, else open progress modal, `prepare` with cancel token, on success `spawn` + open `game_output` window. Second Play while preparing is ignored (`InstanceBusy` → status).

Game output window: append `LogLine` for that `InstanceId`. Closing it does not `kill`.

- [ ] **Step 1: Write failing redact + prepare tests**

```rust
#[test]
fn redact_access_token_query() {
    assert_eq!(
        redact_line("foo accessToken=sekrit bar"),
        "foo accessToken=[redacted] bar"
    );
}

#[test]
fn redact_jwt() {
    let line = "token eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.aaa.bbb";
    assert!(!redact_line(line).contains("eyJ"));
}

#[tokio::test]
async fn prepare_vanilla_offline_errors_without_account() {
    let engine = test_engine().await;
    let id = engine.create_instance(CreateInstance {
        name: "V".into(),
        minecraft_version: "1.21.1".into(),
        loader: Loader::Vanilla,
        loader_version: None,
        icon_png: None,
    }).await.unwrap();
    let err = engine.prepare(id, &NoopProgress, CancellationToken::new(), None).await.unwrap_err();
    assert!(matches!(err, EngineError::NoAccount));
}

#[tokio::test]
async fn prepare_fabric_not_yet() {
    let id = /* create Fabric instance */;
    let err = engine.prepare(...).await.unwrap_err();
    assert!(matches!(err, EngineError::LoaderUnavailable { loader: Loader::Fabric, .. }));
}
```

`NoopProgress` implements `ProgressSink` as empty `set`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib redact:: tests::prepare_vanilla`

Expected: FAIL compile.

- [ ] **Step 3: Implement redact, prepare (vanilla), spawn, kill, UI wiring**

Manifest URL constant:

`pub const VERSION_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";`

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

Run: `cargo run` — Play without account shows sign-in; with a real CLIENT_ID + account, prepare downloads and launches (manual).

- [ ] **Step 5: Commit**

```bash
git add crates/engine src
git commit -m "feat: prepare and spawn vanilla Minecraft"
```

---

### Task 16: Fabric merge

**Files:**
- Create: `crates/engine/src/fabric/mod.rs`
- Create: `crates/engine/tests/fixtures/fabric_loader.json`
- Create: `crates/engine/tests/fixtures/fabric_profile.json`
- Modify: `crates/engine/src/launch/mod.rs` (call fabric merge before downloads)
- Test: `crates/engine/src/fabric/mod.rs`

**Interfaces:**
- Consumes: `VersionInfo`, `Loader::Fabric`, `loader_version`
- Produces:
  - `pub struct FabricLoaderIndex(pub Vec<FabricLoaderEntry>)` from `https://meta.fabricmc.net/v2/versions/loader`
  - `pub fn pick_loader_version(index: &FabricLoaderIndex, preferred: Option<&str>) -> Result<String, EngineError>` — preferred if present, else first `stable == true`, else first entry, else `LoaderUnavailable`
  - `pub fn profile_url(mc: &str, loader: &str) -> String` → `https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/profile/json`
  - `pub fn merge_fabric(vanilla: VersionInfo, profile: FabricProfile) -> VersionInfo` — append profile libraries (url + name → maven path), set `mainClass` from profile

Maven path from `group:artifact:version`: `group/replaced/. /artifact/version/artifact-version.jar`.

- [ ] **Step 1: Write failing tests using fixtures**

`fabric_loader.json`:

```json
[
  { "separator": "+", "build": 1, "maven": "net.fabricmc:fabric-loader:0.16.0", "version": "0.16.0", "stable": true },
  { "separator": "+", "build": 2, "maven": "net.fabricmc:fabric-loader:0.15.0", "version": "0.15.0", "stable": false }
]
```

`fabric_profile.json` minimal: `mainClass` `net.fabricmc.loader.impl.launch.knot.KnotClient`, one library `net.fabricmc:fabric-loader:0.16.0` with url `https://maven.fabricmc.net/`.

```rust
#[test]
fn pick_stable_loader() {
    let idx = load_loader_index();
    assert_eq!(pick_loader_version(&idx, None).unwrap(), "0.16.0");
    assert_eq!(pick_loader_version(&idx, Some("0.15.0")).unwrap(), "0.15.0");
}

#[test]
fn merge_replaces_main_class_and_adds_lib() {
    let v = merge_fabric(load_version("version_1_21.json"), load_profile());
    assert_eq!(v.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
    assert!(v.libraries.iter().any(|l| l.name.contains("fabric-loader")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib fabric::`

Expected: FAIL compile.

- [ ] **Step 3: Implement Fabric types + merge; wire `prepare` for `Loader::Fabric`**

After merge, the rest of `prepare` is unchanged (java, libs, assets, args). Still `Add` the vanilla client jar to classpath.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: merge Fabric loader into the launch plan"
```

---

### Task 17: Forge installer merge + processors

**Files:**
- Create: `crates/engine/src/forge/mod.rs`
- Create: `crates/engine/src/forge/processors.rs`
- Create: `crates/engine/tests/fixtures/forge_install_profile.json`
- Create: `crates/engine/tests/fixtures/forge_version.json`
- Modify: `crates/engine/src/launch/mod.rs`
- Test: `crates/engine/src/forge/mod.rs`

**Interfaces:**
- Consumes: vanilla `VersionInfo`, instance `loader_version`, `HttpFiles`, java path
- Produces:
  - Maven metadata URL: `https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml`
  - `pub fn pick_forge_version(mc: &str, versions: &[String], preferred: Option<&str>) -> Result<String, EngineError>` — preferred or newest `"{mc}-*"`
  - Installer URL: `https://maven.minecraftforge.net/net/minecraftforge/forge/{ver}/forge-{ver}-installer.jar`
  - `pub fn read_installer(jar: &Path) -> Result<(ForgeInstallProfile, VersionInfo), EngineError>` — zip entries `install_profile.json` and `version.json`
  - `pub fn merge_forge(vanilla: VersionInfo, forge_version: VersionInfo) -> VersionInfo` — append libraries; replace `mainClass`; append `arguments` if present (same as Mojang inherit)
  - `pub async fn run_processors(java: &Path, profile: &ForgeInstallProfile, paths: &LauncherPaths, vanilla_client: &Path, cancel) -> Result<(), EngineError>`
    - Skip processors whose `sides` is only `server`
    - Build classpath from processor `jar` + `classpath` maven coords under `cache/libraries`
    - Main class = `Main-Class` of the processor jar manifest
    - Substitute data: `{MINECRAFT_JAR}` → vanilla client path; `{SIDE}` → `client`; `{INSTALLER}` → installer jar; `{ROOT}` → `cache/libraries`; other `{KEY}` → `profile.data[KEY].client` (if starts with `[coord]` resolve to library path; if `/path` relative to a temp extract dir)
    - Run `java -cp <cp> <main> <args>` **unsandboxed**; non-zero exit → `EngineError::AuthFailed` is wrong. Use `EngineError::Http` is wrong.

Add variant already in spec? There is no `ForgeProcessor`. Map processor failure to:

```rust
#[error("auth failed: {message}")]
AuthFailed { message: String }
```

No — that lies. Add nothing to the UI enum. Use:

```rust
EngineError::Io { path: processor_jar, source: io::Error::new(Other, format!("forge processor exited {code}")) }
```

`prepare` for Forge: download installer (sha1 from `.jar.sha1` URL if present), read, download installer libraries, `run_processors`, `merge_forge`, continue as vanilla.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn pick_newest_forge_for_mc() {
    let vs = vec!["1.20.1-47.1.0".into(), "1.20.1-47.2.0".into(), "1.19.4-45.0.0".into()];
    assert_eq!(pick_forge_version("1.20.1", &vs, None).unwrap(), "1.20.1-47.2.0");
    assert_eq!(pick_forge_version("1.20.1", &vs, Some("1.20.1-47.1.0")).unwrap(), "1.20.1-47.1.0");
    assert!(pick_forge_version("1.18.2", &vs, None).is_err());
}

#[test]
fn merge_forge_overrides_main_class() {
    let merged = merge_forge(load_version("version_1_21.json"), load_version("forge_version.json"));
    assert_eq!(merged.main_class, "cpw.mods.bootstraplauncher.BootstrapLauncher");
}

#[test]
fn substitute_processor_args() {
    let out = subst_arg("{MINECRAFT_JAR}", &data, Path::new("/c.jar"), Path::new("/inst.jar"));
    assert_eq!(out, "/c.jar");
}
```

`forge_version.json` fixture: `id` `1.21.1-forge-52.0.0`, `mainClass` `cpw.mods.bootstraplauncher.BootstrapLauncher`, `inheritsFrom` ignored (we already have vanilla), one library.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib forge::`

Expected: FAIL compile.

- [ ] **Step 3: Implement metadata parse (xml via `quick-xml` or regex on `<version>` tags), installer read, merge, processor subst + spawn**

Parse maven-metadata.xml by scanning `<version>...</version>` tags with `quick-xml::Reader`. Add `quick-xml` to `kmine-engine` dependencies.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine
git commit -m "feat: install Forge and merge it into the launch plan"
```

---

### Task 18: Content tab + instance settings UI

**Files:**
- Create: `crates/engine/src/content.rs`
- Modify: `crates/engine/src/lib.rs` (`list_content`, `set_content_enabled`, `delete_content`, `update_instance` already exists)
- Create: `src/screens/instance_content.rs`
- Create: `src/screens/instance_settings.rs`
- Modify: `src/app.rs` (tabs)
- Test: `crates/engine/src/content.rs`

**Interfaces:**
- Consumes: `LauncherPaths::instance_minecraft`, `ContentFolder`, `InstancePatch`
- Produces:
  - `ContentFolder::dir_name() -> &'static str` = `mods` / `resourcepacks` / `shaderpacks`
  - `Engine::list_content` — `read_dir` one level, skip names starting with `.`, skip directories
  - `enabled` = `!file_name.ends_with(".disabled")`
  - `name` = file name without trailing `.disabled`
  - `set_content_enabled(..., true)`: if ends with `.disabled`, rename strip suffix; already enabled → Ok
  - `set_content_enabled(..., false)`: if not disabled, rename add `.disabled`; if target exists → `EngineError::io` with `AlreadyExists`
  - `delete_content`: `remove_file` only if `path.starts_with(folder)` (reject path escape)

Settings tab writes `InstancePatch` for RAM, `jvm_flags`, `java_path`, `sandbox` (checkbox enabled iff `sandbox_status() == Available`; until Task 19 treat Linux without bwrap as Unavailable, macOS/Windows Available), `account_uuid`.

- [ ] **Step 1: Write failing content tests**

```rust
#[test]
fn enable_disable_rename() {
    let root = tempfile::tempdir().unwrap();
    let paths = LauncherPaths::new(root.path().to_path_buf());
    // create instance dirs + mods/sodium.jar
    set_content_enabled_on_disk(... false).unwrap();
    assert!(mods.join("sodium.jar.disabled").is_file());
    set_content_enabled_on_disk(... true).unwrap();
    assert!(mods.join("sodium.jar").is_file());
}

#[test]
fn list_skips_dotfiles_and_dirs() {
    // .DS_Store and subdir ignored
}

#[test]
fn delete_rejects_escape() {
    let err = engine.delete_content(id, Path::new("/etc/passwd")).unwrap_err();
    assert!(matches!(err, EngineError::Io { .. }));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib content::`

Expected: FAIL compile.

- [ ] **Step 3: Implement content.rs + Engine methods + two UI tabs**

Play | Content | Settings switcher on the right pane.

- [ ] **Step 4: Run tests and `cargo run`**

Expected: `cargo test -p kmine-engine` PASS. Dropping a jar into `mods/` shows it; toggle renames.

- [ ] **Step 5: Commit**

```bash
git add crates/engine src
git commit -m "feat: manage local instance content and settings"
```

---

### Task 19: NBT + Quick Play

**Files:**
- Create: `crates/engine/src/nbt.rs`
- Create: `crates/engine/tests/fixtures/servers.dat` (binary, generated in test if easier)
- Modify: `crates/engine/src/lib.rs` (`list_quick_play`)
- Modify: `crates/engine/src/launch/mod.rs` (feature flags + placeholders)
- Modify: `src/screens/instance_play.rs`
- Test: `crates/engine/src/nbt.rs`

**Interfaces:**
- Consumes: instance `.minecraft`
- Produces:
  - `pub fn parse_servers_dat(bytes: &[u8]) -> Result<Vec<QuickPlayServer>, EngineError>`
    - gzip-or-raw NBT compound, list `servers` of compounds with `name` + `ip` strings
  - `pub fn read_level_name(level_dat: &[u8]) -> Option<String>` — `Data.LevelName`
  - `Engine::list_quick_play`:
    - worlds: each `saves/<folder>/level.dat` exists; label = LevelName or folder
    - servers: parse `servers.dat` if present, else empty
  - `prepare(..., Some(QuickPlay::World { folder }))` sets `features.quick_play_single` and `quick_play_singleplayer = folder`
  - `prepare(..., Some(QuickPlay::Server { address }))` sets `quick_play_multi` and `quick_play_multiplayer = address`

- [ ] **Step 1: Write failing NBT tests**

Do not check in an opaque binary you cannot explain. In the test, **write** a minimal uncompressed NBT compound with `quick_xml`-style manual bytes **or** implement `write_servers_dat` next to the parser and round-trip:

```rust
#[test]
fn servers_round_trip() {
    let bytes = encode_servers(&[QuickPlayServer { name: "Hypixel".into(), address: "mc.hypixel.net".into() }]);
    let parsed = parse_servers_dat(&bytes).unwrap();
    assert_eq!(parsed[0].name, "Hypixel");
    assert_eq!(parsed[0].address, "mc.hypixel.net");
}

#[test]
fn level_name_from_compound() {
    let bytes = encode_level_name("My World");
    assert_eq!(read_level_name(&bytes).as_deref(), Some("My World"));
}
```

Implement `encode_*` in `#[cfg(test)]` using the same NBT writer as the reader (tag types: compound `0x0a`, list `0x09`, string `0x08`, end `0x00`; Java modified UTF-8 for strings; big-endian).

Also accept gzip (`0x1f 0x8b`) via `flate2::read::GzDecoder`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib nbt::`

Expected: FAIL compile.

- [ ] **Step 3: Implement NBT reader/writer, `list_quick_play`, prepare wiring, Play tab lists**

Clicking a world/server starts prepare with that `QuickPlay`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine src
git commit -m "feat: parse saves and servers.dat for Quick Play"
```

---

### Task 20: Sandbox backends + LaunchPlan whitelist

**Files:**
- Create: `crates/engine/src/sandbox/mod.rs`
- Create: `crates/engine/src/sandbox/macos.rs`
- Create: `crates/engine/src/sandbox/linux.rs`
- Create: `crates/engine/src/sandbox/windows.rs`
- Modify: `crates/engine/src/launch/mod.rs` (`spawn` uses sandbox)
- Modify: `crates/engine/src/lib.rs` (`sandbox_status`)
- Test: `crates/engine/src/sandbox/mod.rs`

**Interfaces:**
- Consumes: `LaunchPlan`, `SandboxSpec`
- Produces:
  - `pub fn sandbox_status() -> SandboxStatus`
    - macOS / Windows: `Available`
    - Linux: `Available` if `which bwrap` succeeds, else `Unavailable { reason: "bwrap not found on PATH" }`
  - `pub fn fill_spec(plan: &LaunchPlan, paths: &LauncherPaths) -> SandboxSpec` — write: `plan.cwd`, `plan.natives_dir`, plus Linux `paths.root.join("cache/xdg-<instance>")`; read: java parent/home, `cache_libraries`, `cache_assets_objects`, `cache_assets_indexes`, `cache_assets_virtual`, `cache_runtime`, `plan.java`; `network: true`; `enabled` copied from plan
  - `pub fn spawn_sandboxed(plan: &LaunchPlan) -> Result<std::process::Child, EngineError>`
  - macOS: write a Seatbelt profile **you author** from Apple `sandbox_init` docs: `(version 1)`, `(deny default)`, import system graphics, allow read subpaths in `allow_read`, allow read/write on `allow_write`, allow network if `network`, allow exec of `plan.java`. Call `sandbox_init_with_parameters` in a pre-exec hook (`CommandExt::pre_exec`). Do **not** paste Pandora profile text.
  - Linux: `bwrap --ro-bind` reads, `--bind` writes, `--dev-bind /dev/dri` if present, `--proc /proc`, `--dev /dev`, `--unshare-all` + `--share-net` if network, then `java ...`. Missing bwrap → `SandboxUnavailable`.
  - Windows: AppContainer via `CreateAppContainerProfile` named `kmine.<slug-hash>`, grant ACLs on allow paths, network capabilities. If APIs fail → `SandboxUnavailable`.
  - `spawn`: if `!enabled` unsandboxed Command; if enabled && status unavailable → `SandboxUnavailable`; else `spawn_sandboxed`.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn fill_spec_write_set_is_only_game_and_natives() {
    let paths = LauncherPaths::new(PathBuf::from("/data/kmine"));
    let plan = LaunchPlan {
        java: PathBuf::from("/data/kmine/cache/runtime/java-runtime-delta/mac-os-arm64/bin/java"),
        jvm_args: vec![],
        main_class: "n.m.Main".into(),
        game_args: vec![],
        classpath: vec![],
        natives_dir: PathBuf::from("/data/kmine/cache/natives/aaa"),
        cwd: PathBuf::from("/data/kmine/instances/A/.minecraft"),
        env: vec![],
        sandbox: SandboxSpec { enabled: true, allow_read: vec![], allow_write: vec![], network: true },
    };
    let spec = fill_spec(&plan, &paths);
    assert!(spec.allow_write.iter().all(|p| p.starts_with("/data/kmine/instances/A/.minecraft")
        || p.starts_with("/data/kmine/cache/natives")
        || p.starts_with("/data/kmine/cache/xdg-")));
    assert!(!spec.allow_write.iter().any(|p| p.ends_with("kmine.db")));
    assert!(spec.allow_read.iter().any(|p| p.starts_with("/data/kmine/cache/libraries")));
}

#[test]
fn sandbox_status_on_this_os() {
    let s = sandbox_status();
    #[cfg(target_os = "linux")]
    { let _ = s; }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    { assert!(matches!(s, SandboxStatus::Available)); }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-engine --lib sandbox::`

Expected: FAIL compile.

- [ ] **Step 3: Implement fill_spec, status, three backends, wire spawn**

Settings checkbox already exists; it now actually jails the process.

On macOS, after implementation, run a smoke: create instance, enable sandbox, Play. If Seatbelt denies windowing, widen **only** the documented graphics/IOKit allowances, not `allow default`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine src
git commit -m "feat: optional OS sandbox around the game process"
```

---

## Self-review

**Spec coverage**

| Spec item | Task |
|---|---|
| Workspace + engine crate | 1 |
| `kmine.db` schema, config kv | 2 |
| AES-GCM + master key + keychain | 3–4 |
| Accounts + selected_account | 5, 7, 13–14 |
| Instances + slugs + folders | 6–8 |
| GPUI shell / create | 8 |
| Mojang JSON / rules / argv | 9 |
| Downloads sha1 / libs / assets / client | 10–11 |
| Java custom + Mojang runtime | 12 |
| Microsoft OAuth + refresh | 13–14 |
| prepare / spawn / Play / progress / logs / redact | 15 |
| Fabric | 16 |
| Forge | 17 |
| Content + settings | 18 |
| Quick Play + NBT | 19 |
| Sandbox | 20 |
| No store / packs / NeoForge / offline / sync / skins | never scheduled |
| Own CLIENT_ID, bind 47821, keychain names | 13, 4 |
| Tokens never plaintext | 4, 13, 15 |

**Gaps closed in this review:** `EngineError` stays the spec set; Forge processor failures use `Io`. Redact lives in `crates/engine/src/redact.rs` (UI consumes already-redacted `LogLine`). `src/redact.rs` is not created.

**Type names locked:** `InstanceId`, `AccountId`, `Loader`, `Engine`, `EngineHandle`, `Event`, `CreateInstance`, `InstancePatch`, `LaunchPlan`, `SandboxSpec`, `HttpFiles`, `Store`, `Keychain`, `MemoryKeychain`, `OsKeychain`, `ProgressSink`, `QuickPlay`, `ContentFolder`.
