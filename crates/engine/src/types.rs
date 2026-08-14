use crate::ids::{AccountId, InstanceId, Loader};

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
