# kmine-curseforge Core Client Design

Date: 2026-08-15
Status: drafted from conversation

## Goal

Turn `crates/curseforge` (`kmine-curseforge`) into the Minecraft CurseForge Core wrapper: search, categories, projects, files, fingerprints, pack manifests, and file bytes. The operator (later: `kmine-engine`) writes those bytes to disk. This crate never does.

This spec does not add a GPUI store, does not create instances, and does not amend the 2026-08-14 launcher product scope. It specifies the crate a later store/install spec will call.

Wire format source of truth: [docs/reverse/curseforge-api.md](../../reverse/curseforge-api.md). App/agent layout: [docs/reverse/curseforge.md](../../reverse/curseforge.md).

## Decisions (locked)

| Decision | Choice |
|---|---|
| Shape | Domain client, not a 1:1 path mirror and not a full Overwolf SDK |
| Auth | Caller passes the Core `x-api-key`. Crate does not bake, fetch, or extract a key for `Client` |
| Disk | None. Downloads return `bytes::Bytes`. Pack overrides are yielded as `(relative_path, Bytes)` |
| Game | Minecraft only. `gameId` is the constant `432`, not a method argument |
| Account | No CF user Bearer, no SSO, no `app-search`, no share-codes, no comments, no favorites |
| Engine | `kmine-engine` does not depend on this crate in this spec |
| Key extract | Existing `extract_*` + `cf-key` bin stay as they are |
| Runtime | Async. Caller owns the Tokio runtime. Dropping a future cancels the request |

## Scope

### In

- Typed async client for the launcher Core set in the reverse doc §9, plus fingerprints and pack-zip helpers
- Search with class, categories, game versions, loaders, sort, pagination
- Project + batch projects, HTML description
- File list / one file / batch files, HTML changelog
- Minecraft version list and loader index + one loader
- Download a `File` as bytes, with SHA-1 check when the API gave a SHA-1
- CDN URL construction when `downloadUrl` is null
- `manifest.json` parse; in-memory pack zip walk (manifest + overrides)
- Resolve a pack manifest to `File` records via batch
- Resolve required file dependencies (relation type 3)
- CurseForge fingerprint (whitespace-stripped MurmurHash2) and `POST /v1/fingerprints/432`
- Minecraft class / loader / sort / release / relation / hash enums and constants

### Out

- Anything that creates, opens, or writes a filesystem path
- `kmine-engine` methods, SQLite, instances, `ContentFolder` mapping in code
- GPUI / in-app browser
- CF account OIDC, refresh, `app-search`, share-codes, highlights, comments, favorites, servers
- Non-Minecraft `gameId`
- Installing a loader or Minecraft itself (engine already does Fabric/Forge)
- Writing `minecraftinstance.json`
- Baking the Overwolf key into source
- Rate-limit token bucket beyond the official retry policy

## Architecture

Two surfaces, one crate. No third.

```
operator (engine / tests / bin)
    │  api_key + SearchQuery / File / zip bytes
    ▼
Client  ── GET/POST ──►  https://api.curseforge.com
    │
    ├── Page<Mod>, Mod, File, Category, Manifest, ResolvedPack
    └── Downloaded { bytes } / PackOverride { bytes }
              (never a Path)
```

```
crates/curseforge/src/
  lib.rs            re-exports
  asar.rs           existing
  dmg.rs            existing
  extract.rs        existing
  fetch.rs          existing (blocking, cf-key only)
  bin/cf-key.rs     existing
  client.rs         Client, ClientBuilder, transport, retries
  error.rs          Error
  types.rs          wire types + enums + constants
  search.rs         SearchQuery, FileFilter, CategoryFilter
  download.rs       URL resolve, Downloaded
  manifest.rs       Manifest, PackZip
  fingerprint.rs    fingerprint() + match types
```

`kmine-curseforge` must not depend on `kmine-engine`. `reqwest` stays rustls. The catalog client is async (`json`, `stream`). The existing key extractor keeps `reqwest` `blocking` for `cf-key`.

`Client` is `Clone + Send + Sync`. `reqwest::Client` is cloned; the key is cloned with it. `Debug` for `Client` must not print the key (`finish_non_exhaustive`). `Error` display must not include the key.

## Transport

Base URL: `https://api.curseforge.com`, overridable on the builder (wiremock).

Every Core JSON request (host `api.curseforge.com`) sends:

```
x-api-key: <caller key>
Accept: application/json
User-Agent: kmine-curseforge/<CARGO_PKG_VERSION>
```

CDN downloads (`edge.forgecdn.net`, `mediafilez.forgecdn.net`, `modloaders.forgecdn.net`) send **only** that User-Agent. No `x-api-key`, no `Accept: application/json`, no `Authorization`.

Default timeouts: connect 15s, API request 30s, download 300s. Builder can override each.

Retries, matching the official client: `requestRetries = 2`, `delayBetweenRetries = 1000` (fixed). Retry when:

- HTTP 429 — honor `Retry-After` if it is a delay-seconds integer; otherwise 1000ms
- status `<= 199` or `>= 500`

Do not retry 400, 401, 403, 404, 413. After the last failure, return `Error::Http`.

Envelope: most endpoints return `{ "data": T, "pagination"?: Pagination }`. The client unwraps `data`. List endpoints return `Page<T> { data, pagination }`.

Paths the official app uses (v2 where the app uses v2):

| Method | Path |
|---|---|
| GET | `/v1/categories` |
| GET | `/v2/mods/search` |
| GET | `/v2/mods/{modId}` |
| POST | `/v2/mods/get-mods-by-ids` body `{ "modIds": [...] }` |
| GET | `/v1/mods/{modId}/description` |
| GET | `/v1/mods/{modId}/files` |
| GET | `/v1/mods/{modId}/files/{fileId}` |
| POST | `/v1/mods/files` body `{ "fileIds": [...] }` |
| GET | `/v1/mods/{modId}/files/{fileId}/changelog` |
| GET | `/v1/minecraft/version` |
| GET | `/v1/minecraft/version/{gameVersionString}` |
| GET | `/v1/minecraft/modloader?includeAll=true` |
| GET | `/v1/minecraft/modloader/{modLoaderName}` |
| POST | `/v1/fingerprints/432` body `{ "fingerprints": [...] }` |

`GET /v1/mods/{modId}/files/{fileId}/download-url` is a fallback only when both `File.downloadUrl` and the constructed CDN URL fail. Not a public method.

Array query params are JSON array **strings**, not CSV, not repeated keys:

```
categoryIds=[4736,4475]
gameVersions=["1.20.1"]
modLoaderTypes=[1,4]
```

`reqwest` `.query()` is enough — it percent-encodes `[`, `]`, `"`. Do not hand-build query strings. Omit a param when the builder field is `None` or empty.

Limits, enforced before the request (`Error::InvalidQuery` with a static reason):

| Field | Rule |
|---|---|
| `pageSize` | 1..=50 |
| `index + pageSize` | `<= 10000` |
| `categoryIds` | max 10 |
| `gameVersions` | max 4 |
| `modLoaderTypes` | max 5 |
| batch `modIds` / `fileIds` | any length; client chunks at 100 and concatenates |

Default search `pageSize` is 20 (what the official client sends). Default `index` is 0.

## Public API

```rust
pub const MINECRAFT_GAME_ID: u32 = 432;
pub const DEFAULT_BASE_URL: &str = "https://api.curseforge.com";
pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 50;
pub const MAX_INDEX_PLUS_PAGE: u32 = 10_000;
pub const BATCH_SIZE: usize = 100;

pub struct ClassId(pub u32);

impl ClassId {
    pub const BUKKIT_PLUGINS: Self = Self(5);
    pub const MODS: Self = Self(6);
    pub const RESOURCE_PACKS: Self = Self(12);
    pub const WORLDS: Self = Self(17);
    pub const MODPACKS: Self = Self(4471);
    pub const CUSTOMIZATION: Self = Self(4546);
    pub const ADDONS: Self = Self(4559);
    pub const SHADERS: Self = Self(6552);
    pub const DATA_PACKS: Self = Self(6945);
}
```

`ClassId` is a newtype, not an enum, so an unknown class from the API still round-trips. Constants cover the Minecraft classes in the reverse doc.

### Client

```rust
pub struct Client { /* private */ }

pub struct ClientBuilder { /* private */ }

impl Client {
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error>;
    pub fn builder() -> ClientBuilder;

    pub async fn categories(&self, filter: CategoryFilter) -> Result<Vec<Category>, Error>;
    pub async fn search(&self, query: &SearchQuery) -> Result<Page<Mod>, Error>;
    pub async fn get_mod(&self, mod_id: u32) -> Result<Mod, Error>;
    pub async fn get_mods(&self, mod_ids: &[u32]) -> Result<Vec<Mod>, Error>;
    pub async fn description(&self, mod_id: u32) -> Result<String, Error>;

    pub async fn files(&self, mod_id: u32, filter: &FileFilter) -> Result<Page<File>, Error>;
    pub async fn get_file(&self, mod_id: u32, file_id: u32) -> Result<File, Error>;
    pub async fn get_files(&self, file_ids: &[u32]) -> Result<Vec<File>, Error>;
    pub async fn changelog(&self, mod_id: u32, file_id: u32) -> Result<String, Error>;

    pub async fn minecraft_versions(&self) -> Result<Vec<MinecraftVersion>, Error>;
    pub async fn minecraft_version(&self, version: &str) -> Result<MinecraftVersion, Error>;
    pub async fn modloaders(&self) -> Result<Vec<ModLoaderIndexEntry>, Error>;
    pub async fn modloader(&self, name: &str) -> Result<ModLoaderInfo, Error>;

    pub async fn fingerprints(&self, fingerprints: &[u32]) -> Result<FingerprintMatches, Error>;

    pub async fn download(&self, file: &File) -> Result<Downloaded, Error>;
    pub async fn resolve_pack(&self, manifest: &Manifest) -> Result<ResolvedPack, Error>;
    pub async fn resolve_required_deps(
        &self,
        roots: &[File],
        game_version: &str,
        loader: Option<ModLoaderType>,
    ) -> Result<Vec<File>, Error>;
}

impl ClientBuilder {
    pub fn api_key(self, key: impl Into<String>) -> Self;
    pub fn base_url(self, url: impl Into<String>) -> Self;
    pub fn user_agent(self, ua: impl Into<String>) -> Self;
    pub fn connect_timeout(self, d: Duration) -> Self;
    pub fn request_timeout(self, d: Duration) -> Self;
    pub fn download_timeout(self, d: Duration) -> Self;
    pub fn build(self) -> Result<Client, Error>;
}
```

`Client::new(key)` is `Client::builder().api_key(key).build()`. Empty key is `Error::InvalidQuery` ("empty api key") — fail before any HTTP.

HTTP 404 mapping:

- `get_mod`, `description` → `Error::NotFound { kind: Mod, id: mod_id }`
- `get_file`, `changelog` → `Error::NotFound { kind: File, id: file_id }`
- everything else, including `minecraft_version` and `modloader` (string keys, not ids) → `Error::Http { url, status: 404 }`

`get_mods` / `get_files` preserve no order. Callers that care must index by id. Empty input returns `Ok(vec![])` without HTTP.

`description` and `changelog` return the HTML string inside `data`.

### Search and filters

```rust
pub struct SearchQuery { /* private fields, built via methods */ }

impl SearchQuery {
    pub fn new(class: ClassId) -> Self; // gameId always 432
    pub fn search(self, text: impl Into<String>) -> Self;
    pub fn categories(self, ids: impl Into<Vec<u32>>) -> Self;
    pub fn category(self, id: u32) -> Self;
    pub fn game_versions(self, versions: impl Into<Vec<String>>) -> Self;
    pub fn game_version(self, v: impl Into<String>) -> Self;
    pub fn loaders(self, loaders: impl Into<Vec<ModLoaderType>>) -> Self;
    pub fn loader(self, loader: ModLoaderType) -> Self;
    pub fn sort(self, field: SortField, order: SortOrder) -> Self;
    pub fn slug(self, slug: impl Into<String>) -> Self;
    pub fn author_id(self, id: u32) -> Self;
    pub fn game_version_type_id(self, id: u32) -> Self;
    pub fn index(self, index: u32) -> Self;
    pub fn page_size(self, page_size: u32) -> Self;
}

impl Default for SearchQuery {
    fn default() -> Self { SearchQuery::new(ClassId::MODS) }
}

pub enum CategoryFilter {
    All,
    ClassesOnly,
    ChildrenOf(ClassId),
}

pub struct FileFilter {
    pub game_version: Option<String>,
    pub game_version_type_id: Option<u32>,
    pub loader: Option<ModLoaderType>,
    pub client_compatible: Option<bool>,
    pub index: u32,
    pub page_size: u32,
}

impl Default for FileFilter {
    fn default() -> Self {
        Self {
            game_version: None,
            game_version_type_id: None,
            loader: None,
            client_compatible: None,
            index: 0,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
}
```

`SearchQuery` default sort is `SortField::Popularity` + `SortOrder::Desc` (official search). `FileFilter` list uses a single `modLoaderType` (the files endpoint does not take the array form).

`Page<T>`:

```rust
pub struct Page<T> {
    pub data: Vec<T>,
    pub pagination: Pagination,
}

pub struct Pagination {
    pub index: u32,
    pub page_size: u32,
    pub result_count: u32,
    pub total_count: u32,
}

impl Pagination {
    /// Next `index`, or None at the end or if the next window would pass 10000.
    pub fn next_index(&self) -> Option<u32>;
}
```

`next_index` is `index + page_size` when `index + page_size < total_count` and `index + 2 * page_size <= 10000`; else `None`. Callers that need every file loop `FileFilter.index = page.next_index()?`.

## Types

Serde: `rename_all = "camelCase"`, unknown fields ignored, missing fields `#[serde(default)]`. Timestamps stay `String` (RFC3339 as the API sent them). No `chrono` dependency.

Enums are `#[repr(u8)]` (or `u16` if needed) and serde as integers, matching the reverse doc. Unknown integers must not fail deserialize: use `#[serde(try_from = "u8")]` **or** keep a newtype. Decision: newtypes for anything CF may extend (`FileStatus`), closed enums for the documented tables below. Unknown `ModLoaderType` / `SortField` / `FileRelationType` / `HashAlgo` / `FileReleaseType` deserialize as `Other(u8)` so a new loader does not break search.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModLoaderType {
    Any,        // 0
    Forge,      // 1
    Cauldron,   // 2
    LiteLoader, // 3
    Fabric,     // 4
    Quilt,      // 5
    NeoForge,   // 6
    Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortField {
    Featured,          // 1
    Popularity,        // 2
    LastUpdated,       // 3
    Name,              // 4
    Author,            // 5
    TotalDownloads,    // 6
    Category,          // 7
    GameVersion,       // 8
    EarlyAccess,       // 9
    FeaturedReleased,  // 10
    ReleasedDate,      // 11
    Rating,            // 12
    Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortOrder { Asc, Desc } // "asc" / "desc"

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileReleaseType {
    Release, // 1
    Beta,    // 2
    Alpha,   // 3
    Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileRelationType {
    EmbeddedLibrary,     // 1
    OptionalDependency,  // 2
    RequiredDependency,  // 3
    Tool,                // 4
    Incompatible,        // 5
    Include,             // 6
    Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgo {
    Sha1, // 1
    Md5,  // 2
    Other(u8),
}
```

Each closed-with-`Other` enum implements `from_u8` / `as_u8` used by serde. `SortField` wire value is the table above (already +1 vs the official client's internal 0-based enum). Do not add 1 again.

`FileStatus` is `u32`. Approved is `4`. Do not enum it.

Core structs (fields the crate guarantees; extra API keys are dropped):

```rust
pub struct Category {
    pub id: u32,
    pub game_id: u32,
    pub name: String,
    pub slug: String,
    pub url: Option<String>,
    pub icon_url: Option<String>,
    pub date_modified: Option<String>,
    pub is_class: bool,
    pub class_id: Option<u32>,
    pub parent_category_id: Option<u32>,
    pub display_index: Option<i32>,
}

pub struct Mod {
    pub id: u32,
    pub game_id: u32,
    pub name: String,
    pub slug: String,
    pub links: ModLinks,
    pub summary: String,
    pub status: u32,
    pub download_count: u64,
    pub is_featured: bool,
    pub primary_category_id: Option<u32>,
    pub categories: Vec<Category>,
    pub class_id: Option<u32>,
    pub authors: Vec<ModAuthor>,
    pub logo: Option<ModAsset>,
    pub screenshots: Vec<ModAsset>,
    pub main_file_id: Option<u32>,
    pub latest_files: Vec<File>,
    pub latest_files_indexes: Vec<FileIndex>,
    pub date_created: Option<String>,
    pub date_modified: Option<String>,
    pub date_released: Option<String>,
    pub allow_mod_distribution: Option<bool>,
    pub game_popularity_rank: Option<u32>,
    pub is_available: bool,
    pub thumbs_up_count: u64,
}

pub struct ModLinks {
    pub website_url: Option<String>,
    pub wiki_url: Option<String>,
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
}

pub struct ModAuthor {
    pub id: u32,
    pub name: String,
    pub url: Option<String>,
}

pub struct ModAsset {
    pub id: u32,
    pub mod_id: Option<u32>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub url: Option<String>,
}

pub struct FileIndex {
    pub game_version: String,
    pub file_id: u32,
    pub filename: String,
    pub release_type: FileReleaseType,
    pub game_version_type_id: Option<u32>,
    pub mod_loader: Option<ModLoaderType>,
}

pub struct File {
    pub id: u32,
    pub game_id: u32,
    pub mod_id: u32,
    pub is_available: bool,
    pub display_name: String,
    pub file_name: String,
    pub release_type: FileReleaseType,
    pub file_status: u32,
    pub hashes: Vec<FileHash>,
    pub file_date: Option<String>,
    pub file_length: u64,
    pub download_count: u64,
    pub download_url: Option<String>,
    pub game_versions: Vec<String>,
    pub sortable_game_versions: Vec<SortableGameVersion>,
    pub dependencies: Vec<FileDependency>,
    pub alternate_file_id: Option<u32>,
    pub is_server_pack: bool,
    pub server_pack_file_id: Option<u32>,
    pub is_early_access_content: bool,
    pub file_fingerprint: u32,
    pub modules: Vec<FileModule>,
}

pub struct FileHash {
    pub algo: HashAlgo,
    pub value: String,
}

pub struct FileDependency {
    pub mod_id: u32,
    pub relation_type: FileRelationType,
}

pub struct FileModule {
    pub name: String,
    pub fingerprint: u32,
}

pub struct SortableGameVersion {
    pub game_version_name: Option<String>,
    pub game_version_padded: Option<String>,
    pub game_version: Option<String>,
    pub game_version_release_date: Option<String>,
    pub game_version_type_id: Option<u32>,
}

pub struct MinecraftVersion {
    pub id: Option<u32>,
    pub game_version_id: Option<u32>,
    pub version_string: String,
    pub jar_download_url: Option<String>,
    pub json_download_url: Option<String>,
    pub approved: Option<bool>,
    pub date_modified: Option<String>,
    pub game_version_type_id: Option<u32>,
}

pub struct ModLoaderIndexEntry {
    pub name: String,
    pub game_version: String,
    pub latest: bool,
    pub recommended: bool,
    pub date_modified: Option<String>,
    pub loader_type: Option<ModLoaderType>, // wire name: "type"
}

pub struct ModLoaderInfo {
    pub name: String,
    pub game_version: String,
    pub latest: bool,
    pub recommended: bool,
    pub download_url: Option<String>,
    pub filename: Option<String>,
    pub install_method: Option<i32>,
    pub libraries_install_location: Option<String>,
    pub version_json: Option<serde_json::Value>,
    pub install_profile_json: Option<serde_json::Value>,
}
```

`ModLoaderIndexEntry.loader_type` reads JSON `type`. `version_json` / `install_profile_json` stay `Value` — Forge profile shape is engine's problem.

Helpers on the types (no HTTP):

```rust
impl File {
    pub fn sha1(&self) -> Option<&str>; // algo Sha1, first
    pub fn md5(&self) -> Option<&str>;
    pub fn is_approved(&self) -> bool; // is_available && file_status == 4
    pub fn required_mod_ids(&self) -> impl Iterator<Item = u32> + '_;
}

impl Mod {
    /// First `latest_files_indexes` row (API order) for which `FileIndex::matches` is true.
    pub fn file_index_for(&self, mc: &str, loader: Option<ModLoaderType>) -> Option<&FileIndex>;
}

impl FileIndex {
    /// `game_version` equals `mc`. Loader:
    /// - `None` (caller does not care) → any row
    /// - `Some(Any)` → any row
    /// - `Some(Forge|…)` → row.mod_loader is that value, `Any`, or `None`
    pub fn matches(&self, mc: &str, loader: Option<ModLoaderType>) -> bool;
}
```

`file_index_for` scans in API order (CF already sorts newest-first in that array). No second guess by filename.

## Download

```rust
pub struct Downloaded {
    pub file_id: u32,
    pub file_name: String,
    pub bytes: Bytes,
    pub sha1: Option<String>, // lowercase hex actually hashed
}

/// Official CDN split: 5754631 → .../files/5754/631/{fileName}
pub fn cdn_file_url(file_id: u32, file_name: &str) -> String;

pub fn resolve_download_url(file: &File) -> Option<String>;
```

`cdn_file_url` is:

```
https://edge.forgecdn.net/files/{file_id / 1000}/{file_id % 1000}/{url_encoded_file_name}
```

`5754631` + `oreexcavation-1.13.174.jar` →
`https://edge.forgecdn.net/files/5754/631/oreexcavation-1.13.174.jar`.

`resolve_download_url`:

1. If `file.download_url` is `Some` and non-empty, use it.
2. Else if `file_name` is non-empty, use `cdn_file_url`.
3. Else `None`.

`Client::download`:

1. Resolve URL. `None` → `Error::NoDownloadUrl { mod_id, file_id }`.
2. GET the URL **without** `x-api-key`, using `download_timeout`.
3. On HTTP failure of a `download_url` that was not already the CDN URL, retry once against `cdn_file_url` (then normal 429/5xx retries on that URL).
4. Collect the body into `Bytes`.
5. If `file.sha1()` is `Some`, SHA-1 the bytes. Mismatch → `Error::ChecksumMismatch`. Match → put the lowercase hex in `Downloaded.sha1`.
6. If no SHA-1 was advertised, still return the bytes and set `sha1` to the computed hash (operator can store it).

The crate does not check MD5. The crate does not write a `.part` file.

`allow_mod_distribution == Some(false)` does **not** refuse the download. The first-party key the operator is expected to pass often still works; if the CDN returns 403 the operator sees `Error::Http`. Policy lives above this crate.

## Packs

CurseForge pack zip, in memory.

```rust
pub struct Manifest {
    pub minecraft: ManifestMinecraft,
    pub manifest_type: String,
    pub manifest_version: u32,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub overrides: String, // default "overrides"
    pub files: Vec<ManifestFile>,
}

pub struct ManifestMinecraft {
    pub version: String,
    pub mod_loaders: Vec<ManifestLoader>,
}

pub struct ManifestLoader {
    pub id: String,      // "forge-47.4.0", "fabric-0.16.9-1.21.1"
    pub primary: bool,
}

pub struct ManifestFile {
    pub project_id: u32, // projectID
    pub file_id: u32,    // fileID
    pub required: bool,  // default true
}

impl Manifest {
    pub fn parse(json: &[u8]) -> Result<Self, Error>;
    pub fn primary_loader(&self) -> Option<&ManifestLoader>;
}

pub struct PackZip { /* ZipArchive<Cursor<Bytes>> */ }

pub struct PackOverride {
    pub relative_path: String, // relative to the overrides folder, `/` separators
    pub bytes: Bytes,
}

impl PackZip {
    pub fn parse(bytes: impl Into<Bytes>) -> Result<Self, Error>;
    pub fn manifest(&mut self) -> Result<Manifest, Error>;
    /// Next file under the overrides prefix. Skip directories.
    /// Paths are relative to that prefix (`config/foo.toml`, not `overrides/config/foo.toml`).
    pub fn next_override(&mut self) -> Option<Result<PackOverride, Error>>;
}

pub struct ResolvedPack {
    pub manifest: Manifest,
    pub files: Vec<ResolvedPackFile>,
}

pub struct ResolvedPackFile {
    pub project_id: u32,
    pub file_id: u32,
    pub required: bool,
    pub file: File,
}
```

`Manifest::parse` requires `manifestType == "minecraftModpack"` (case-sensitive) and `minecraft.version` non-empty. Anything else is `Error::Manifest`.

`PackZip::parse` opens the zip from memory (`zip` crate, deflate). `manifest()` reads `manifest.json` at the zip root (not under a wrapper folder). If the only top-level entry is a directory and `manifest.json` sits inside it, accept that one-level prefix so Finder-zipped packs still work. Two or more roots without a root `manifest.json` is `Error::Manifest`.

`next_override` walks remaining zip entries whose path is under `manifest.overrides` (default `"overrides"`), after the same optional prefix. It does not preload every override. SkyFactory-sized packs (~8k override paths) stay lazy. Callers write each `PackOverride` and drop it.

`Client::resolve_pack`:

1. Collect `manifest.files` ids.
2. `get_files` in chunks of `BATCH_SIZE`.
3. For each manifest row, find the `File` with that `file_id`. Missing required file → `Error::NotFound { kind: File, id }`. Missing optional (`required == false`) → omit from `files`.
4. Return `ResolvedPack`. This does not download jars. Every required manifest row is present in `files`.

`Client::resolve_required_deps`:

1. `seen` is a set of **mod ids**. Insert every `roots[i].mod_id`.
2. Queue each `required_mod_ids()` from the roots that is not in `seen`.
3. Pop `mod_id`, insert it into `seen` immediately (cycle break), then `get_mod` → `file_index_for(game_version, loader)` → `get_file`. No index → `Error::NoCompatibleFile { mod_id, game_version }`.
4. Push that `File` onto the output, enqueue its required dep mod ids that are not in `seen`.
5. Stop when the queue is empty. Return only the new files (not the roots).

Cap the walk at 512 fetched files. Above that: `Error::InvalidQuery` ("dependency walk exceeded 512").

### Operator recipe (not crate code)

This is what engine will do later. The crate exposes each step, not a single `install`.

1. `get_mod(pack_id)` — operator checks `class_id == Some(ClassId::MODPACKS.0)`
2. Pick `file_id` from `file_index_for` or the user
3. `get_file` + `download` → pack zip bytes
4. `PackZip::parse` → `manifest()`
5. `resolve_pack` → each `ResolvedPackFile.file` → `download`
6. `modloader(primary_loader.id)` — engine installs that loader
7. `next_override` loop — engine writes under the instance
8. For a single mod (not a pack): `download` + `resolve_required_deps` + download those too

How those bytes land on `mods/` / `resourcepacks/` / `shaderpacks/` is engine mapping, not this crate. Hint for that later spec:

| `ClassId` | Typical dest |
|---|---|
| `MODS` | `.minecraft/mods/` |
| `RESOURCE_PACKS` | `.minecraft/resourcepacks/` |
| `SHADERS` | `.minecraft/shaderpacks/` |
| `WORLDS` | `.minecraft/saves/` |
| `DATA_PACKS` | world `datapacks/` |
| `MODPACKS` | zip, then the recipe above |

## Fingerprints

CurseForge file fingerprint, not a generic Murmur.

1. Drop bytes `9` (tab), `10` (LF), `13` (CR), `32` (space).
2. MurmurHash2, 32-bit, little-endian blocks, seed `1`, `m = 0x5bd1e995`, `r = 24`.
3. Return `u32`. This is `File.file_fingerprint` / instance `packageFingerprint`.

```rust
pub fn fingerprint(bytes: &[u8]) -> u32;
```

Golden vectors (must pass):

| Input | Output |
|---|---:|
| `b""` | `1540447798` |
| `b" \t\r\n"` | `1540447798` |
| `b"a"` | `626045324` |
| `b"abcd"` | `3376380438` |
| `b"hello"` | `2788266382` |
| `b"he llo\n"` | `2788266382` |

```rust
pub struct FingerprintMatches {
    pub exact: Vec<FingerprintMatch>,
    pub unmatched: Vec<u32>,
}

pub struct FingerprintMatch {
    pub id: u32,          // file id
    pub file: File,
    pub latest_files: Vec<File>,
}
```

`Client::fingerprints` POSTs `{ "fingerprints": [...] }` to `/v1/fingerprints/432`. Map `data.exactMatches` → `exact` (`id`, `file`, `latestFiles`). Map `data.unmatchedFingerprints` → `unmatched` (default empty). Empty input → `Ok` empty, no HTTP.

## Errors

```rust
pub enum Error {
    Http { url: String, status: u16 },
    NotFound { kind: ResourceKind, id: u32 },
    NoDownloadUrl { mod_id: u32, file_id: u32 },
    NoCompatibleFile { mod_id: u32, game_version: String },
    ChecksumMismatch { file_id: u32, expected: String, actual: String },
    Decode { url: String, message: String },
    Manifest { message: String },
    Zip { message: String },
    InvalidQuery { message: &'static str },
    Builder { message: &'static str },
}

pub enum ResourceKind { Mod, File }
```

`thiserror` on `Error`. `CfKeyError` stays separate and is **not** a variant of `Error` — extract and catalog do not share a flow.

`Client::new` / `build` fail with `Error::Builder` on reqwest build failure or empty key (`InvalidQuery` for empty key, as above).

No `Cancelled` variant. Cancellation is dropping the future.

## Cargo

Workspace member already listed. Package name stays `kmine-curseforge`.

Add (versions aligned with `kmine-engine` where the same crate exists):

- `bytes`
- `reqwest` — keep current, add features `json`, `stream` (keep `blocking`, `rustls-tls`)
- `sha1`, `hex` — SHA-1 of downloaded bytes
- `tokio` — `rt` only if a test runtime is needed; library code does not start a runtime. Tests use `tokio` with `macros`, `rt-multi-thread` as dev-dep
- `tokio-util` — not required
- existing: `regex`, `serde`, `serde_json`, `thiserror`, `udif`, `zip`

Dev-dep: `wiremock`, `tempfile` (extract tests already want a tree; catalog tests should not need tempfile).

Do not add `chrono`, `kmine-engine`, or `murmur3`. Implement MurmurHash2 in `fingerprint.rs` (~40 lines). Do not pull a generic hash crate that is easy to mis-seed.

## Tests

No live `api.curseforge.com` in default `cargo test`. Wiremock + fixtures only.

| Test | Asserts |
|---|---|
| `types` JEI-shaped `Mod` fixture | `id`, `slug`, `class_id`, `latest_files_indexes[0].mod_loader == Forge` |
| `types` file `5754631` fixture from the reverse doc | `file_name`, `file_fingerprint == 3871571640`, `sha1()`, `required_mod_ids` |
| `SearchQuery` encoding | mock observes `gameId=432`, `classId=6`, `gameVersions` is a JSON array string, `modLoaderTypes` same, `pageSize=20` |
| `SearchQuery` rejects `page_size=51` | `InvalidQuery` |
| `categories` | unwraps `{ data: [...] }` |
| `get_mod` 404 | `Error::NotFound { kind: Mod, .. }` |
| `get_files` chunks 101 ids into 2 POSTs | body lengths 100 and 1 |
| `cdn_file_url(5754631, "oreexcavation-1.13.174.jar")` | exact edge.forgecdn.net URL above |
| `download` uses `downloadUrl` | GET that URL, no `x-api-key` header |
| `download` null URL | GET constructed CDN |
| `download` SHA-1 mismatch | `ChecksumMismatch`, no `Downloaded` |
| retry | first 503 then 200 → Ok |
| no retry on 404 | single request |
| `Manifest::parse` SkyFactory-shaped JSON | version, primary loader id, one file |
| `PackZip` with root `manifest.json` + `overrides/config/a.txt` | manifest name; one override `config/a.txt` |
| `PackZip` wrapped in one folder | same |
| `fingerprint` table | six golden vectors |
| `resolve_required_deps` | root file depends on mod 2; mock mod+file; returns one new `File`, not the root |
| `Client` `Debug` | formatted string does not contain the key |

Keep existing extract tests (`pulls_key_from_asar_js`, optional `CurseForge.app` check).

Fixtures live in `crates/curseforge/tests/fixtures/` as JSON copied from the reverse doc (JEI skeleton, file 5754631, pack manifest). Do not check in a real 20MB pack zip; build tiny zips in the test.

## Relationship to the rest of kmine

- `kmine` (GPUI) does not import this crate in this spec.
- `kmine-engine` does not import this crate in this spec.
- A later spec will: take a key (env / settings / one-time extract), construct `Client`, search, download bytes, write them next to `content.rs`, create instances from `ResolvedPack` + `ModLoaderInfo`.
- Until that spec, the 2026-08-14 launcher doc remains correct: no in-app store.

## Key decisions

1. **Caller-supplied key** — extract stays a tool. The library is usable with any Core key the operator obtained.
2. **Bytes, not paths** — one rule, no "just this helper writes a cache".
3. **Minecraft-only domain API** — `SearchQuery::new(ClassId)` instead of raw paths. Less room to send `gameId=1` by accident.
4. **v2 where the official app uses v2** — search and get-mod on `/v2/...`.
5. **JSON-array query strings** — CF rejects CSV for `categoryIds` / `gameVersions` / `modLoaderTypes`.
6. **CDN fallback, no distribution policy** — crate tries the same URLs the official client tries; 403 is data, not a special type.
7. **Pack walk is lazy** — manifest is small; overrides can be thousands of files.
8. **MurmurHash2 in-tree** — seed and whitespace filter are too easy to get wrong behind a generic crate.
9. **No Bearer in v1** — search and download of mods/packs do not need a CF account.
10. **Engine stays unaware** — this crate can land and be tested without touching launch/auth/UI.

## Implementation slices

Each slice is independently reviewable and leaves `cargo test -p kmine-curseforge` green.

1. **Types + fingerprint + manifest JSON** — no HTTP. Fixtures and golden hashes.
2. **Client transport + categories/search/get_mod/get_mods/description** — wiremock.
3. **Files + changelog + minecraft versions/loaders** — wiremock.
4. **Download + CDN + SHA-1 + retries** — wiremock.
5. **PackZip + resolve_pack + resolve_required_deps + fingerprints()** — wiremock + tiny zip.

Existing extract/asar/dmg/`cf-key` is not part of these slices unless a change is required to compile (feature flags on `reqwest`). Prefer adding features on the same `reqwest` dep over splitting crates.
