pub mod auth;
pub mod catalog;
pub mod content;
pub mod error;
pub mod fabric;
pub mod forge;
pub mod http;
pub mod ids;
pub mod instance;
pub mod java;
pub mod launch;
pub mod logfmt;
pub mod mojang;
pub mod nbt;
pub mod neoforge;
pub mod paths;
pub mod quilt;
pub mod redact;
pub mod sandbox;
pub mod skin;
pub mod store;
pub mod types;

pub use catalog::{
    CatalogCategory, CatalogError, CatalogFile, CatalogFileFilter, CatalogPage, CatalogProject,
    CatalogProjectDetail, CatalogProjectId, CatalogProvider, CatalogQuery, CatalogSort,
    ContentClass, ProviderId, parse_manifest_loader,
};
pub use error::EngineError;
pub use http::HttpFiles;
pub use ids::{AccountId, InstanceId, Loader};
pub use launch::VERSION_MANIFEST_URL;
pub use paths::LauncherPaths;
pub use redact::redact_line;
pub use store::{Keychain, MemoryKeychain, OsKeychain};
pub use tokio_util::sync::CancellationToken;
pub use types::{
    AccountRecord, AccountSummary, ContentEntry, ContentFolder, CreateInstance, GameProcessId,
    InstancePatch, InstanceRow, InstanceSummary, LaunchPlan, PrepareMode, ProgressSink, QuickPlay,
    QuickPlayLists, QuickPlayServer, QuickPlayWorld, SandboxSpec, SandboxStatus,
};

use crate::instance::{
    create_instance_dirs, delete_instance_dir, rename_instance_dir, slug_from_name, unique_slug,
};
use crate::store::Store;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

pub struct Engine {
    pub(crate) paths: LauncherPaths,
    pub(crate) store: Arc<parking_lot::Mutex<Store>>,
    pub(crate) master_key: [u8; 32],
    pub(crate) events: tokio::sync::broadcast::Sender<Event>,
    pub(crate) processes: Arc<parking_lot::Mutex<HashMap<InstanceId, Running>>>,
    pub(crate) preparing: parking_lot::Mutex<HashSet<InstanceId>>,
    pub(crate) redact_tokens: parking_lot::Mutex<HashMap<InstanceId, Vec<String>>>,
    pub(crate) login_lock: tokio::sync::Mutex<bool>,
    pub(crate) login_cancel: parking_lot::Mutex<Option<CancellationToken>>,
    pub(crate) rt: tokio::runtime::Handle,
    pub(crate) providers: Arc<parking_lot::Mutex<Vec<Arc<dyn catalog::CatalogProvider>>>>,
    pub(crate) catalog_backend_url: Arc<parking_lot::Mutex<String>>,
    pub(crate) catalog_backend_token: Option<String>,
    pub(crate) installing: parking_lot::Mutex<bool>,
}

pub struct Running {
    pub(crate) kill: tokio::sync::watch::Sender<bool>,
    pub(crate) started_at: Instant,
}

#[derive(Debug, Clone)]
pub enum Event {
    InstancesChanged,
    AccountsChanged,
    Progress {
        id: InstanceId,
        title: String,
        done: u64,
        total: u64,
    },
    PrepareFinished {
        id: InstanceId,
        ok: bool,
    },
    LogLine {
        instance_id: InstanceId,
        stream: LogStream,
        text: String,
    },
    ProcessExited {
        instance_id: InstanceId,
        code: Option<i32>,
    },
    AuthRequired,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl Engine {
    pub async fn open(paths: LauncherPaths) -> Result<Self, EngineError> {
        Self::from_keychain(paths, &OsKeychain)
    }

    #[cfg(test)]
    pub fn open_with_keychain(
        paths: LauncherPaths,
        kc: &dyn Keychain,
    ) -> Result<Self, EngineError> {
        Self::from_keychain(paths, kc)
    }

    fn from_keychain(paths: LauncherPaths, kc: &dyn Keychain) -> Result<Self, EngineError> {
        paths.create_dirs()?;
        let (store, master_key) = Store::open(&paths.db, kc)?;
        let (events, _) = tokio::sync::broadcast::channel(512);
        Ok(Self {
            paths,
            store: Arc::new(parking_lot::Mutex::new(store)),
            master_key,
            events,
            processes: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            preparing: parking_lot::Mutex::new(HashSet::new()),
            redact_tokens: parking_lot::Mutex::new(HashMap::new()),
            login_lock: tokio::sync::Mutex::new(false),
            login_cancel: parking_lot::Mutex::new(None),
            rt: tokio::runtime::Handle::current(),
            providers: Arc::new(parking_lot::Mutex::new(Vec::new())),
            catalog_backend_url: Arc::new(parking_lot::Mutex::new(
                catalog::key::default_catalog_backend_url(),
            )),
            catalog_backend_token: catalog::key::catalog_backend_token_from_env(),
            installing: parking_lot::Mutex::new(false),
        })
    }

    pub fn event_sender(&self) -> tokio::sync::broadcast::Sender<Event> {
        self.events.clone()
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub fn list_accounts(&self) -> Result<Vec<AccountSummary>, EngineError> {
        let store = self.store.lock();
        let selected = store.selected_account()?;
        Ok(store
            .list_accounts()?
            .into_iter()
            .map(|rec| AccountSummary {
                uuid: rec.uuid,
                username: rec.username,
                selected: selected == Some(rec.uuid),
            })
            .collect())
    }

    pub fn list_instances(&self) -> Result<Vec<InstanceSummary>, EngineError> {
        let rows = self.store.lock().list_instances()?;
        let processes = self.processes.lock();
        Ok(rows
            .into_iter()
            .map(|row| {
                let icon = cache_instance_icon(&self.paths, &row);
                InstanceSummary {
                    id: row.id,
                    slug: row.slug,
                    name: row.name,
                    minecraft_version: row.minecraft_version,
                    loader: row.loader,
                    last_played_at: row.last_played_at,
                    playtime_secs: row.playtime_secs as u64,
                    running: processes.contains_key(&row.id),
                    icon,
                }
            })
            .collect())
    }

    pub fn get_instance(&self, id: InstanceId) -> Result<Option<InstanceRow>, EngineError> {
        self.store.lock().get_instance(id)
    }

    pub fn sandbox_status(&self) -> SandboxStatus {
        crate::sandbox::sandbox_status()
    }

    pub fn library_dir(&self) -> &std::path::Path {
        &self.paths.root
    }

    pub async fn create_instance(&self, spec: CreateInstance) -> Result<InstanceId, EngineError> {
        let store = self.store.lock();
        let slug = unique_slug(&slug_from_name(&spec.name), &store.list_slugs()?);
        create_instance_dirs(&self.paths, &slug)?;
        let id = InstanceId::new();
        let row = InstanceRow {
            id,
            slug: slug.clone(),
            name: spec.name,
            minecraft_version: spec.minecraft_version,
            loader: spec.loader,
            loader_version: spec.loader_version,
            account_uuid: None,
            memory_min_mb: None,
            memory_max_mb: None,
            jvm_flags: None,
            java_path: None,
            sandbox: false,
            icon_png: spec.icon_png,
            created_at: now_ms(),
            last_played_at: None,
            playtime_secs: 0,
            session_count: 0,
        };
        if let Err(err) = store.insert_instance(&row) {
            let _ = delete_instance_dir(&self.paths, &slug);
            return Err(err);
        }
        drop(store);
        self.emit(Event::InstancesChanged);
        Ok(id)
    }

    pub async fn rename_instance(&self, id: InstanceId, name: String) -> Result<(), EngineError> {
        let store = self.store.lock();
        let Some(mut row) = store.get_instance(id)? else {
            return Err(instance_not_found(&self.paths));
        };
        let others: Vec<String> = store
            .list_slugs()?
            .into_iter()
            .filter(|s| s != &row.slug)
            .collect();
        let new_slug = unique_slug(&slug_from_name(&name), &others);
        if new_slug != row.slug {
            rename_instance_dir(&self.paths, &row.slug, &new_slug)?;
        }
        row.name = name;
        row.slug = new_slug;
        store.update_instance(&row)?;
        drop(store);
        self.emit(Event::InstancesChanged);
        Ok(())
    }

    pub async fn delete_instance(&self, id: InstanceId) -> Result<(), EngineError> {
        let store = self.store.lock();
        if let Some(row) = store.get_instance(id)? {
            delete_instance_dir(&self.paths, &row.slug)?;
        }
        store.delete_instance(id)?;
        let mut pinned = store.pinned_instances()?;
        if let Some(i) = pinned.iter().position(|pinned_id| *pinned_id == id) {
            pinned.remove(i);
            store.set_pinned_instances(&pinned)?;
        }
        drop(store);
        self.emit(Event::InstancesChanged);
        Ok(())
    }

    pub fn pinned_instances(&self) -> Result<Vec<InstanceId>, EngineError> {
        self.store.lock().pinned_instances()
    }

    pub fn toggle_instance_pin(&self, id: InstanceId) -> Result<bool, EngineError> {
        let store = self.store.lock();
        let mut pinned = store.pinned_instances()?;
        let now_pinned = if let Some(i) = pinned.iter().position(|pinned_id| *pinned_id == id) {
            pinned.remove(i);
            false
        } else {
            pinned.push(id);
            true
        };
        store.set_pinned_instances(&pinned)?;
        Ok(now_pinned)
    }

    pub fn reorder_pinned_instance(
        &self,
        dragged: InstanceId,
        target: InstanceId,
    ) -> Result<Vec<InstanceId>, EngineError> {
        let store = self.store.lock();
        let mut pinned = store.pinned_instances()?;
        move_pinned(&mut pinned, dragged, target);
        store.set_pinned_instances(&pinned)?;
        Ok(pinned)
    }

    pub async fn update_instance(
        &self,
        id: InstanceId,
        patch: InstancePatch,
    ) -> Result<(), EngineError> {
        let store = self.store.lock();
        let Some(mut row) = store.get_instance(id)? else {
            return Err(instance_not_found(&self.paths));
        };
        patch.apply(&mut row);
        store.update_instance(&row)?;
        drop(store);
        self.emit(Event::InstancesChanged);
        Ok(())
    }

    pub async fn start_login(&self) -> Result<AccountSummary, EngineError> {
        if crate::auth::client_id().is_empty() {
            return Err(EngineError::AuthNotConfigured);
        }
        let _guard = self
            .login_lock
            .try_lock()
            .map_err(|_| EngineError::LoginInProgress)?;

        let bind = crate::auth::bind_addr();
        let listener =
            std::net::TcpListener::bind(&bind).map_err(|e| EngineError::io(bind.as_str(), e))?;
        let request = crate::auth::oauth::authorize_request()?;
        open::that(request.url.as_str()).map_err(|e| {
            EngineError::io(
                std::path::PathBuf::from("browser"),
                std::io::Error::other(e.to_string()),
            )
        })?;
        let cancel = CancellationToken::new();
        *self.login_cancel.lock() = Some(cancel.clone());
        let callback = tokio::task::spawn_blocking(move || {
            crate::auth::oauth::wait_for_callback(listener, &cancel)
        })
        .await
        .map_err(|e| EngineError::io(bind.as_str(), std::io::Error::other(e.to_string())));
        self.login_cancel.lock().take();
        let callback = callback??;
        if callback.state != request.state {
            return Err(EngineError::AuthFailed {
                message: "oauth state mismatch".into(),
            });
        }

        let http = HttpFiles::new()?;
        let endpoints = crate::auth::AuthEndpoints::production();
        let (record, secrets) =
            crate::auth::complete_login(&http, &callback.code, &request.pkce_verifier, &endpoints)
                .await?;
        let summary = {
            let store = self.store.lock();
            crate::auth::persist_login(&store, &self.master_key, &record, &secrets)?
        };
        self.emit(Event::AccountsChanged);
        Ok(summary)
    }

    pub async fn select_account(&self, id: AccountId) -> Result<(), EngineError> {
        self.store.lock().set_selected_account(Some(id))?;
        self.emit(Event::AccountsChanged);
        Ok(())
    }

    pub fn cancel_login(&self) {
        if let Some(cancel) = self.login_cancel.lock().take() {
            cancel.cancel();
        }
    }

    pub async fn delete_account(&self, id: AccountId) -> Result<(), EngineError> {
        let store = self.store.lock();
        store.delete_account(id)?;
        if store.selected_account()? == Some(id) {
            store.set_selected_account(None)?;
        }
        drop(store);
        self.emit(Event::AccountsChanged);
        Ok(())
    }

    pub(crate) fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

#[derive(Clone)]
pub struct EngineHandle {
    engine: std::sync::Arc<Engine>,
    rt: tokio::runtime::Handle,
}

impl EngineHandle {
    pub fn new(engine: std::sync::Arc<Engine>, rt: tokio::runtime::Handle) -> Self {
        Self { engine, rt }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.rt
    }
}

pub(crate) fn now_ms() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_millis().min(i64::MAX as u128) as i64,
        Err(_) => 0,
    }
}

fn cache_instance_icon(paths: &LauncherPaths, row: &InstanceRow) -> Option<std::path::PathBuf> {
    let bytes = row.icon_png.as_ref().filter(|bytes| !bytes.is_empty())?;
    let dir = paths.root.join("cache").join("instance-icons");
    std::fs::create_dir_all(&dir).ok()?;
    let dest = dir.join(format!("{}.png", row.id.as_hyphenated()));
    let stale = std::fs::metadata(&dest)
        .map(|meta| meta.len() as usize != bytes.len())
        .unwrap_or(true);
    if stale {
        std::fs::write(&dest, bytes).ok()?;
    }
    Some(dest)
}

fn move_pinned(pinned: &mut Vec<InstanceId>, dragged: InstanceId, target: InstanceId) {
    if dragged == target {
        return;
    }
    let Some(from) = pinned.iter().position(|id| *id == dragged) else {
        return;
    };
    let Some(to) = pinned.iter().position(|id| *id == target) else {
        return;
    };
    let item = pinned.remove(from);
    pinned.insert(to, item);
}

pub(crate) fn instance_not_found(paths: &LauncherPaths) -> EngineError {
    EngineError::io(
        paths.instances.clone(),
        std::io::Error::new(std::io::ErrorKind::NotFound, "instance not found"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_rename_delete_instance() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths.clone(), &kc).unwrap();
        let id = engine
            .create_instance(CreateInstance {
                name: "One".into(),
                minecraft_version: "1.21.1".into(),
                loader: Loader::Vanilla,
                loader_version: None,
                icon_png: None,
            })
            .await
            .unwrap();
        assert_eq!(engine.list_instances().unwrap()[0].name, "One");
        assert!(paths.instance_minecraft("One").is_dir());
        engine.rename_instance(id, "Two".into()).await.unwrap();
        assert!(paths.instance_minecraft("Two").is_dir());
        engine.delete_instance(id).await.unwrap();
        assert!(engine.list_instances().unwrap().is_empty());
    }

    fn sample_instance() -> CreateInstance {
        named_instance("One")
    }

    fn named_instance(name: &str) -> CreateInstance {
        CreateInstance {
            name: name.into(),
            minecraft_version: "1.21.1".into(),
            loader: Loader::Vanilla,
            loader_version: None,
            icon_png: None,
        }
    }

    #[test]
    fn move_pinned_places_dragged_at_target() {
        let a = InstanceId::new();
        let b = InstanceId::new();
        let c = InstanceId::new();
        let mut pins = vec![a, b, c];
        move_pinned(&mut pins, a, c);
        assert_eq!(pins, vec![b, c, a]);
        move_pinned(&mut pins, c, b);
        assert_eq!(pins, vec![c, b, a]);
        move_pinned(&mut pins, a, a);
        assert_eq!(pins, vec![c, b, a]);
        move_pinned(&mut pins, InstanceId::new(), b);
        assert_eq!(pins, vec![c, b, a]);
    }

    #[tokio::test]
    async fn pinned_instance_survives_reopen() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths.clone(), &kc).unwrap();
        let id = engine.create_instance(sample_instance()).await.unwrap();
        assert!(engine.pinned_instances().unwrap().is_empty());
        assert!(engine.toggle_instance_pin(id).unwrap());
        assert!(engine.pinned_instances().unwrap().contains(&id));
        drop(engine);

        let engine = Engine::open_with_keychain(paths, &kc).unwrap();
        assert!(engine.pinned_instances().unwrap().contains(&id));
        assert!(!engine.toggle_instance_pin(id).unwrap());
        assert!(engine.pinned_instances().unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_instance_drops_pin() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths, &kc).unwrap();
        let id = engine.create_instance(sample_instance()).await.unwrap();
        engine.toggle_instance_pin(id).unwrap();
        engine.delete_instance(id).await.unwrap();
        assert!(engine.pinned_instances().unwrap().is_empty());
    }

    #[tokio::test]
    async fn pinned_order_survives_reopen_and_reorder() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths.clone(), &kc).unwrap();
        let a = engine.create_instance(named_instance("A")).await.unwrap();
        let b = engine.create_instance(named_instance("B")).await.unwrap();
        let c = engine.create_instance(named_instance("C")).await.unwrap();
        engine.toggle_instance_pin(a).unwrap();
        engine.toggle_instance_pin(b).unwrap();
        engine.toggle_instance_pin(c).unwrap();
        assert_eq!(engine.pinned_instances().unwrap(), vec![a, b, c]);
        assert_eq!(engine.reorder_pinned_instance(a, c).unwrap(), vec![b, c, a]);
        drop(engine);

        let engine = Engine::open_with_keychain(paths, &kc).unwrap();
        assert_eq!(engine.pinned_instances().unwrap(), vec![b, c, a]);
    }

    #[tokio::test]
    async fn update_instance_clears_java_path() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths.clone(), &kc).unwrap();
        let id = engine
            .create_instance(CreateInstance {
                name: "One".into(),
                minecraft_version: "1.21.1".into(),
                loader: Loader::Vanilla,
                loader_version: None,
                icon_png: None,
            })
            .await
            .unwrap();
        let created_at = Store::open_file(&paths.db)
            .unwrap()
            .get_instance(id)
            .unwrap()
            .unwrap()
            .created_at;
        engine
            .update_instance(
                id,
                InstancePatch {
                    java_path: Some(Some(std::path::PathBuf::from("/opt/java"))),
                    memory_min_mb: None,
                    memory_max_mb: None,
                    jvm_flags: None,
                    sandbox: None,
                    account_uuid: None,
                    icon_png: None,
                    minecraft_version: None,
                    loader: None,
                    loader_version: None,
                },
            )
            .await
            .unwrap();
        let row = Store::open_file(&paths.db)
            .unwrap()
            .get_instance(id)
            .unwrap()
            .unwrap();
        assert_eq!(row.java_path.as_deref(), Some("/opt/java"));
        engine
            .update_instance(
                id,
                InstancePatch {
                    java_path: Some(None),
                    memory_min_mb: None,
                    memory_max_mb: None,
                    jvm_flags: None,
                    sandbox: None,
                    account_uuid: None,
                    icon_png: None,
                    minecraft_version: None,
                    loader: None,
                    loader_version: None,
                },
            )
            .await
            .unwrap();
        let row = Store::open_file(&paths.db)
            .unwrap()
            .get_instance(id)
            .unwrap()
            .unwrap();
        assert_eq!(row.java_path, None);
        assert_eq!(row.created_at, created_at);
    }
}
