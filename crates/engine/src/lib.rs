pub mod error;
pub mod http;
pub mod ids;
pub mod instance;
pub mod mojang;
pub mod paths;
pub mod store;
pub mod types;

pub use error::EngineError;
pub use http::HttpFiles;
pub use ids::{AccountId, InstanceId, Loader};
pub use paths::LauncherPaths;
pub use store::{Keychain, MemoryKeychain, OsKeychain};
pub use types::{
    AccountRecord, AccountSummary, CreateInstance, InstancePatch, InstanceRow, InstanceSummary,
    LaunchPlan, ProgressSink, SandboxSpec, SandboxStatus,
};

use crate::instance::{
    create_instance_dirs, delete_instance_dir, rename_instance_dir, slug_from_name, unique_slug,
};
use crate::store::Store;

pub struct Engine {
    paths: LauncherPaths,
    store: parking_lot::Mutex<Store>,
    #[allow(dead_code)]
    master_key: [u8; 32],
    events: tokio::sync::broadcast::Sender<Event>,
    processes: parking_lot::Mutex<std::collections::HashMap<InstanceId, Running>>,
    #[allow(dead_code)]
    login_lock: tokio::sync::Mutex<bool>,
}

pub struct Running;

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
        let (events, _) = tokio::sync::broadcast::channel(64);
        Ok(Self {
            paths,
            store: parking_lot::Mutex::new(store),
            master_key,
            events,
            processes: parking_lot::Mutex::new(std::collections::HashMap::new()),
            login_lock: tokio::sync::Mutex::new(false),
        })
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
            .map(|row| InstanceSummary {
                id: row.id,
                slug: row.slug,
                name: row.name,
                minecraft_version: row.minecraft_version,
                loader: row.loader,
                last_played_at: row.last_played_at,
                playtime_secs: row.playtime_secs as u64,
                running: processes.contains_key(&row.id),
            })
            .collect())
    }

    pub fn sandbox_status(&self) -> SandboxStatus {
        SandboxStatus::Available
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
        drop(store);
        self.emit(Event::InstancesChanged);
        Ok(())
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

    pub async fn select_account(&self, id: AccountId) -> Result<(), EngineError> {
        self.store.lock().set_selected_account(Some(id))?;
        self.emit(Event::AccountsChanged);
        Ok(())
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

    fn emit(&self, event: Event) {
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

fn now_ms() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_millis().min(i64::MAX as u128) as i64,
        Err(_) => 0,
    }
}

fn instance_not_found(paths: &LauncherPaths) -> EngineError {
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
