use crate::ids::Loader;
use crate::types::ContentFolder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentClass {
    Mods,
    ResourcePacks,
    Shaders,
    Modpacks,
}

impl ContentClass {
    pub fn dest_folder(self) -> Option<ContentFolder> {
        match self {
            Self::Mods => Some(ContentFolder::Mods),
            Self::ResourcePacks => Some(ContentFolder::Resourcepacks),
            Self::Shaders => Some(ContentFolder::Shaderpacks),
            Self::Modpacks => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CatalogProjectId(pub String);

#[derive(Debug, Clone)]
pub enum CatalogCredentials {
    ApiKey(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSort {
    Popularity,
    LastUpdated,
    Downloads,
    Name,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogRelease {
    Release,
    Beta,
    Alpha,
    Other,
}

#[derive(Debug, Clone)]
pub struct CatalogQuery {
    pub class: ContentClass,
    pub provider: super::ProviderId,
    pub search: Option<String>,
    pub category_ids: Vec<String>,
    pub game_version: Option<String>,
    pub loader: Option<Loader>,
    pub sort: CatalogSort,
    pub index: u32,
    pub page_size: u32,
}

impl CatalogQuery {
    pub fn page_size_or_default(page_size: u32) -> u32 {
        page_size.clamp(1, 50)
    }
}

#[derive(Debug, Clone)]
pub struct CatalogCategory {
    pub id: String,
    pub name: String,
    pub class: ContentClass,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CatalogProject {
    pub provider: super::ProviderId,
    pub id: CatalogProjectId,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub authors: Vec<String>,
    pub download_count: u64,
    pub logo_url: Option<String>,
    pub class: ContentClass,
}

#[derive(Debug, Clone)]
pub struct CatalogProjectDetail {
    pub project: CatalogProject,
    pub description_html: String,
    pub screenshot_urls: Vec<String>,
    pub website_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CatalogFile {
    pub provider: super::ProviderId,
    pub project_id: CatalogProjectId,
    pub class: ContentClass,
    pub file_id: String,
    pub display_name: String,
    pub file_name: String,
    pub release: CatalogRelease,
    pub game_versions: Vec<String>,
    pub loaders: Vec<Loader>,
    pub file_length: u64,
    pub download_count: u64,
    pub file_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CatalogFileFilter {
    pub game_version: Option<String>,
    pub loader: Option<Loader>,
    pub index: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone)]
pub struct CatalogBlob {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub sha1: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CatalogPage<T> {
    pub items: Vec<T>,
    pub index: u32,
    pub page_size: u32,
    pub total: u32,
}

#[derive(Debug, Clone)]
pub struct PackManifestSpec {
    pub name: String,
    pub version: String,
    pub minecraft_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub files: Vec<PackManifestFileSpec>,
}

#[derive(Debug, Clone)]
pub struct PackManifestFileSpec {
    pub project_id: CatalogProjectId,
    pub file_id: String,
    pub required: bool,
    pub class: ContentClass,
}

#[derive(Debug, Clone)]
pub struct PackOverride {
    pub relative_path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogResource {
    Project,
    File,
    Category,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog unavailable")]
    Unavailable,
    #[error("unknown catalog provider")]
    UnknownProvider,
    #[error("catalog {kind:?} not found: {id}")]
    NotFound { kind: CatalogResource, id: String },
    #[error("catalog http {status} for {url}")]
    Http { url: String, status: u16 },
    #[error("unsupported mod loader: {raw}")]
    UnsupportedLoader { raw: String },
    #[error("catalog manifest error: {message}")]
    Manifest { message: String },
    #[error("catalog checksum mismatch for {file_id}: expected {expected}, got {actual}")]
    Checksum {
        file_id: String,
        expected: String,
        actual: String,
    },
    #[error("{0}")]
    Message(String),
}
