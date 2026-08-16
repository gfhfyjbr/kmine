use crate::ids::{AccountId, InstanceId, Loader};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentFolder {
    Mods,
    Resourcepacks,
    Shaderpacks,
}

impl ContentFolder {
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Mods => "mods",
            Self::Resourcepacks => "resourcepacks",
            Self::Shaderpacks => "shaderpacks",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentEntry {
    pub path: PathBuf,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    pub uuid: AccountId,
    pub username: String,
    pub added_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSummary {
    pub uuid: AccountId,
    pub username: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceRow {
    pub id: InstanceId,
    pub slug: String,
    pub name: String,
    pub minecraft_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub account_uuid: Option<AccountId>,
    pub memory_min_mb: Option<i64>,
    pub memory_max_mb: Option<i64>,
    pub jvm_flags: Option<String>,
    pub java_path: Option<String>,
    pub sandbox: bool,
    pub icon_png: Option<Vec<u8>>,
    pub created_at: i64,
    pub last_played_at: Option<i64>,
    pub playtime_secs: i64,
    pub session_count: i64,
}

#[derive(Debug, Clone)]
pub struct CreateInstance {
    pub name: String,
    pub minecraft_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub icon_png: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
pub struct InstancePatch {
    pub memory_min_mb: Option<Option<u32>>,
    pub memory_max_mb: Option<Option<u32>>,
    pub jvm_flags: Option<Option<String>>,
    pub java_path: Option<Option<PathBuf>>,
    pub sandbox: Option<bool>,
    pub account_uuid: Option<Option<AccountId>>,
    pub icon_png: Option<Option<Vec<u8>>>,
    pub minecraft_version: Option<String>,
    pub loader: Option<Loader>,
    pub loader_version: Option<Option<String>>,
}

impl InstancePatch {
    pub(crate) fn apply(self, row: &mut InstanceRow) {
        if let Some(v) = self.memory_min_mb {
            row.memory_min_mb = v.map(i64::from);
        }
        if let Some(v) = self.memory_max_mb {
            row.memory_max_mb = v.map(i64::from);
        }
        if let Some(v) = self.jvm_flags {
            row.jvm_flags = v;
        }
        if let Some(v) = self.java_path {
            row.java_path = v.map(|p| p.to_string_lossy().into_owned());
        }
        if let Some(v) = self.sandbox {
            row.sandbox = v;
        }
        if let Some(v) = self.account_uuid {
            row.account_uuid = v;
        }
        if let Some(v) = self.icon_png {
            row.icon_png = v;
        }
        if let Some(v) = self.minecraft_version {
            row.minecraft_version = v;
        }
        if let Some(v) = self.loader {
            row.loader = v;
        }
        if let Some(v) = self.loader_version {
            row.loader_version = v;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceSummary {
    pub id: InstanceId,
    pub slug: String,
    pub name: String,
    pub minecraft_version: String,
    pub loader: Loader,
    pub last_played_at: Option<i64>,
    pub playtime_secs: u64,
    pub running: bool,
    /// Cached cover PNG when the instance has a custom icon.
    pub icon: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxStatus {
    Available,
    Unavailable { reason: String },
}

pub trait ProgressSink: Send + Sync {
    fn set(&self, title: &str, done: u64, total: u64);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickPlay {
    World { folder: String },
    Server { address: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuickPlayLists {
    pub worlds: Vec<QuickPlayWorld>,
    pub servers: Vec<QuickPlayServer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickPlayWorld {
    pub folder: String,
    pub label: String,
    /// Last in-game world preview (`saves/<folder>/icon.png`), if present.
    pub icon: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickPlayServer {
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameProcessId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareMode {
    Warm,
    Verify,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub java: PathBuf,
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
    pub classpath: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub cwd: PathBuf, // instances/<slug>/.minecraft
    pub env: Vec<(String, String)>,
    pub sandbox: SandboxSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSpec {
    pub enabled: bool,
    pub allow_read: Vec<PathBuf>,
    pub allow_write: Vec<PathBuf>,
    pub network: bool, // always true for Minecraft
}
