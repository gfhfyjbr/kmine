pub mod args;
pub mod assets;
pub mod libraries;
pub mod rules;

pub use args::{ArgContext, build_args, interpolate, join_classpath};
pub use assets::{AssetsRoot, fetch_assets};
pub use libraries::{
    LibraryArtifact, extract_natives, fetch_client, fetch_libraries, natives_dir_name,
    select_libraries,
};
pub use rules::{
    FeatureSet, Rule, RuleAction, RuleFeatures, RuleOs, current_os_arch, current_os_name,
    rule_allows,
};

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    pub main_class: String,
    pub arguments: Option<VersionArguments>,
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    pub asset_index: Option<AssetIndex>,
    pub assets: Option<String>,
    pub downloads: Option<VersionDownloads>,
    pub java_version: Option<JavaVersion>,
    pub logging: Option<Logging>,
    #[serde(default = "default_version_type", rename = "type")]
    pub version_type: String,
}

fn default_version_type() -> String {
    "release".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionArguments {
    #[serde(default)]
    pub game: Vec<LaunchArgument>,
    #[serde(default)]
    pub jvm: Vec<LaunchArgument>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum LaunchArgument {
    Value(String),
    Ruled { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Library {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    pub natives: Option<HashMap<String, String>>,
    pub extract: Option<Extract>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artifact {
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub total_size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionDownloads {
    pub client: Option<Download>,
    pub server: Option<Download>,
    pub client_mappings: Option<Download>,
    pub server_mappings: Option<Download>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Download {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Logging {
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: LoggingFile,
    #[serde(rename = "type")]
    pub log_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}
