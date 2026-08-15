# kmine Catalog Integration Design

Date: 2026-08-15
Status: drafted from conversation; awaiting file review

This spec amends [2026-08-14-kmine-launcher-design.md](2026-08-14-kmine-launcher-design.md): in-app catalog (search / project / file / install) is now in scope, and so are NeoForge and Quilt. It does **not** amend [2026-08-15-curseforge-crate-design.md](2026-08-15-curseforge-crate-design.md): `kmine-curseforge` stays a bytes-only Core client plus the existing key extractor. `kmine-engine` still does not depend on that crate.

Wire format for CurseForge Core: [docs/reverse/curseforge-api.md](../../reverse/curseforge-api.md).

## Goal

Let the user create vanilla / Fabric / Forge / NeoForge / Quilt instances from the existing small sheet, install a CurseForge modpack as a new instance from a catalog modal (search, categories, project, file, install), and add mods / resource packs / shaders to an existing instance from the same catalog. A later provider (Modrinth, etc.) is a second `CatalogProvider`, not a second UI.

The Core `x-api-key` is **not** extracted in the launcher. A separate `backend-api` process extracts it from official-app sources and serves `GET /get_cf_api_key`. The launcher fetches that key every hour and stores it encrypted.

## Decisions (locked)

| Decision | Choice |
|---|---|
| Product shape | Create wizard (kind grid → loader sheet **or** catalog). Content tab **Add** reuses the catalog |
| Who owns catalog HTTP | Engine defines `CatalogProvider`. The `kmine` binary injects `CurseForgeProvider`. UI never imports CF types |
| Merged feed | No. UI sends one `provider` per query (chip). Default `curseforge` |
| Key source | `crates/backend-api` `GET /get_cf_api_key`. Launcher does not run `extract_*` |
| Key cache | Hourly GET. Success writes `secrets.id = catalog/curseforge`. Failure keeps the last secret |
| Catalog depth | List → project → pick file → Install. Required deps yes. Optional deps no |
| Loaders | Add `NeoForge` and `Quilt` to `Loader` and `prepare` in this spec |
| Pack rollback | Failed or cancelled `install_pack` deletes the instance |
| Add rollback | Keep files already written in that batch; stop on first error |
| Provenance / updates | Not in this spec. No `minecraftinstance.json` |
| Worlds / datapacks / Bukkit | Out. `supports` is false |
| Key fetch from Overwolf DMG inside the app | Out (that is `backend-api`, not `kmine`) |

## Scope

### In

- `Loader::{NeoForge, Quilt}` and `prepare` / create-UI for them
- `crates/backend-api`: extract + `GET /get_cf_api_key`
- Engine catalog types, trait, hourly key refresh, `install_pack`, `install_content`, `catalog_*` queries
- `kmine` adapter `src/providers/curseforge.rs` wrapping `kmine-curseforge::Client`
- Create-instance kind grid; catalog modal; Content **Add**
- `cache/catalog/` for pack zips, project files, remote images

### Out

- Extracting or fetching the Core key inside `kmine` / `kmine-engine`
- Optional dependency picker
- Pack/mod updates, changelog compare, pin-to-latest
- Modrinth (or any second provider) implementation
- CF account, favorites, share-codes, comments
- Worlds, datapacks, Bukkit, customization classes
- Merged multi-provider search
- HTML rendering of CF descriptions
- Settings field to paste a key
- GPUI tests

## Architecture

```
                    ┌─────────────────────────────────────┐
                    │ crates/backend-api  (own process)   │
                    │  extract_from_source → memory       │
                    │  GET /get_cf_api_key                │
                    └────────────────┬────────────────────┘
                                     │ hourly + on first miss
                                     ▼
kmine (GPUI)                    kmine-engine
  CreatePhase / CatalogModal      catalog/{types,provider,install,key}
  Content Add ─────────────────►  install_pack / install_content
  providers/curseforge.rs         Loader + prepare (NeoForge, Quilt)
         │                        secrets catalog/curseforge
         ▼                        cache/catalog/files|images
   kmine-curseforge::Client
         │
         ▼
   api.curseforge.com / CDN
```

Dependencies:

| Crate | May depend on | Must not depend on |
|---|---|---|
| `kmine-engine` | (existing) | `kmine-curseforge` |
| `kmine-curseforge` | (existing) | `kmine-engine` |
| `backend-api` | `kmine-curseforge` (`extract_*` only) | `kmine-engine`, catalog `Client` |
| `kmine` | `kmine-engine`, `kmine-curseforge` (adapter only) | extract in the UI path |

`kmine` `main` constructs the engine first, then injects providers, then starts the key loop. `Engine::open` does **not** start refresh (no providers yet).

```rust
let engine = Engine::open(paths).await?;
engine.add_provider(Arc::new(CurseForgeProvider::new()));
engine.start_catalog_key_refresh(); // no-op if provider `curseforge` is missing
```

UI calls only `Engine` / `EngineHandle`. It never constructs `kmine_curseforge::SearchQuery`.

## Crate and module layout

```
crates/engine/src/
  ids.rs                 Loader += NeoForge, Quilt
  catalog/
    mod.rs               re-exports
    types.rs             ContentClass, Catalog*, PackManifestSpec, CatalogError
    provider.rs          CatalogProvider, ProviderId
    query.rs             Engine::catalog_* dispatch
    install.rs           install_pack, install_content, dest mapping, overrides
    key.rs               hourly GET, secrets, set_credentials
    loader_id.rs         parse_manifest_loader
  quilt/mod.rs           meta.quiltmc.org, merge_quilt
  neoforge/mod.rs        maven.neoforged.net, pick + installer; uses forge::run_processors

crates/backend-api/          # package name: kmine-backend-api
  Cargo.toml
  src/main.rs                # axum, bind, token, extract cache, GET /get_cf_api_key

src/providers/curseforge.rs
src/modals/create_instance.rs   kind grid + loader sheet
src/modals/catalog.rs           list / project / files
src/screens/instance_content.rs Add button
```

Workspace `members` adds `crates/backend-api`.

`async_trait` is added to `kmine-engine` so `CatalogProvider` is object-safe (`Vec<Arc<dyn CatalogProvider>>`).

## Catalog types and trait

```rust
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderId(pub &'static str);

impl ProviderId {
    pub const CURSEFORGE: Self = Self("curseforge");
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
    pub provider: ProviderId,
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
```

Default `page_size` is 20. `index + page_size` must be `<= 10000` (adapter returns `CatalogError` / CF `InvalidQuery`). Default sort is `Popularity`.

```rust
pub struct CatalogCategory {
    pub id: String,
    pub name: String,
    pub class: ContentClass,
    pub parent_id: Option<String>,
}

pub struct CatalogProject {
    pub provider: ProviderId,
    pub id: CatalogProjectId,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub authors: Vec<String>,
    pub download_count: u64,
    pub logo_url: Option<String>,
    pub class: ContentClass,
}

pub struct CatalogProjectDetail {
    pub project: CatalogProject,
    pub description_html: String, // stored; UI does not render HTML in this spec
    pub screenshot_urls: Vec<String>,
    pub website_url: Option<String>,
}

pub struct CatalogFile {
    pub provider: ProviderId,
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

pub struct CatalogFileFilter {
    pub game_version: Option<String>,
    pub loader: Option<Loader>,
    pub index: u32,
    pub page_size: u32,
}

pub struct CatalogBlob {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub sha1: Option<String>,
}

pub struct CatalogPage<T> {
    pub items: Vec<T>,
    pub index: u32,
    pub page_size: u32,
    pub total: u32,
}

pub struct PackManifestSpec {
    pub name: String,
    pub version: String,
    pub minecraft_version: String,
    pub loader: Loader,
    pub loader_version: Option<String>,
    pub files: Vec<PackManifestFileSpec>,
}

pub struct PackManifestFileSpec {
    pub project_id: CatalogProjectId,
    pub file_id: String,
    pub required: bool,
    pub class: ContentClass, // from get_mod class_id; required + unknown class is an error
}

pub struct PackOverride {
    pub relative_path: String, // / separators, relative to .minecraft
    pub bytes: Vec<u8>,
}
```

```rust
#[async_trait]
pub trait CatalogProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn label(&self) -> &'static str;
    fn supports(&self, class: ContentClass) -> bool;

    fn set_credentials(&self, creds: CatalogCredentials);
    fn has_credentials(&self) -> bool;

    async fn categories(&self, class: ContentClass) -> Result<Vec<CatalogCategory>, CatalogError>;
    async fn search(&self, query: &CatalogQuery) -> Result<CatalogPage<CatalogProject>, CatalogError>;
    async fn project(&self, id: &CatalogProjectId) -> Result<CatalogProjectDetail, CatalogError>;
    async fn files(
        &self,
        id: &CatalogProjectId,
        filter: &CatalogFileFilter,
    ) -> Result<CatalogPage<CatalogFile>, CatalogError>;
    async fn file(
        &self,
        project_id: &CatalogProjectId,
        file_id: &str,
    ) -> Result<CatalogFile, CatalogError>;
    async fn download(&self, file: &CatalogFile) -> Result<CatalogBlob, CatalogError>;

    async fn parse_pack(&self, zip: &[u8]) -> Result<PackManifestSpec, CatalogError>;
    fn walk_overrides(
        &self,
        zip: &[u8],
        visit: &mut dyn FnMut(PackOverride) -> Result<(), CatalogError>,
    ) -> Result<(), CatalogError>;

    async fn resolve_required_deps(
        &self,
        roots: &[CatalogFile],
        game_version: &str,
        loader: Option<Loader>,
    ) -> Result<Vec<CatalogFile>, CatalogError>;
}
```

`Engine` holds `Vec<Arc<dyn CatalogProvider>>`. `catalog_*` select **one** provider by `ProviderId` (`catalog_search` reads `query.provider`). Unknown id → `CatalogError::UnknownProvider`. User cancel during `install_*` is `EngineError::Cancelled` (existing), not a catalog variant.

```rust
pub enum CatalogError {
    Unavailable, // no usable key
    UnknownProvider,
    NotFound { kind: CatalogResource, id: String },
    Http { url: String, status: u16 },
    UnsupportedLoader { raw: String },
    Manifest { message: String },
    Checksum { file_id: String, expected: String, actual: String },
    Message(String),
}

pub enum CatalogResource { Project, File, Category }

// EngineError gains: Catalog(CatalogError)
```

`Display` / `Debug` for `CatalogError` and `EngineError::Catalog` must not include API keys. Adapter maps `kmine_curseforge::Error` here and redacts.

`parse_manifest_loader` is `pub` on `kmine-engine` (`catalog/loader_id.rs`) so the binary adapter can call it.

`CatalogProvider::set_credentials` takes `&self`. Implementations keep `Mutex<Option<Client>>` (or equivalent). `CurseForgeProvider::new()` starts with no `Client`; the first `set_credentials(ApiKey)` builds `kmine_curseforge::Client`. Later calls replace it. `has_credentials` is true iff that mutex is `Some`.

### CurseForge adapter mapping

Only in `src/providers/curseforge.rs`:

| CF | Neutral |
|---|---|
| `ClassId::MODS` | `Mods` |
| `ClassId::RESOURCE_PACKS` | `ResourcePacks` |
| `ClassId::SHADERS` | `Shaders` |
| `ClassId::MODPACKS` | `Modpacks` |
| worlds, datapacks, Bukkit, customization, addons | `supports == false`; never queried |
| `ModLoaderType::{Forge,Fabric,NeoForge,Quilt}` | `Loader::*` |
| Any / Cauldron / LiteLoader / Other | omitted from `CatalogFile.loaders` |
| `FileReleaseType` 1/2/3 | Release / Beta / Alpha; else `Other` |
| `SortField` popularity / lastUpdated / totalDownloads / name | `CatalogSort` |
| `Mod.id` | `CatalogProjectId(id.to_string())` |
| category `id` | `CatalogCategory.id` decimal string |

`parse_pack`: `PackZip::parse` → `manifest()` → `parse_manifest_loader` → `Client::resolve_pack` → `get_mods` for each distinct `project_id` to fill `class`. Missing class on a **required** file → `CatalogError::Manifest`. Optional manifest rows are dropped (not downloaded).

`walk_overrides` forwards `PackZip::next_override`. Paths stay relative to the overrides folder (`config/foo.toml`).

`resolve_required_deps` wraps `Client::resolve_required_deps` and maps `File` → `CatalogFile`, filling `class` via `get_mod` (already fetched during the walk, or a follow-up `get_mods`). Missing class on a required dep → `Manifest`.

Without credentials the adapter returns `Unavailable` before any HTTP to `api.curseforge.com`.

## `backend-api`

Separate process. Depends on `kmine-curseforge` extract/fetch only (`extract_from_source`). Does not construct `Client`.

| env | default | meaning |
|---|---|---|
| `KMINE_CF_KEY_SOURCE` | (required) | Path to `CurseForge.app` / asar / dmg **or** `http(s)://…` |
| `KMINE_BACKEND_BIND` | `127.0.0.1:8787` | Listen addr |
| `KMINE_BACKEND_TOKEN` | unset | If set, require `Authorization: Bearer <token>` |

On start: `extract_from_source(KMINE_CF_KEY_SOURCE)`, keep `CfCoreKey` in memory. `GET` never re-downloads a DMG. Re-extract when:

- local source `mtime` changes (check before serving, cheap `stat`), or
- `SIGHUP`, or
- source is a URL and 24h have passed since last successful extract

If memory is empty (extract has never succeeded) → `503` with body `{"error":"key unavailable"}` (no key material).

```
GET /get_cf_api_key
Accept: application/json
Authorization: Bearer <token>    # only when KMINE_BACKEND_TOKEN is set
```

`200`:

```json
{ "apiKey": "$2a$10$…", "source": "asar:dist/background/background.js" }
```

`source` is `CfCoreKey.source`. `401` if token configured and missing/wrong. No other routes in this spec.

Logs and error bodies never include `apiKey`. Bind is loopback by default; exposing it is the operator's problem (token + reverse proxy).

The launcher does **not** spawn this process.

## Key refresh in engine

Env on the **client**:

| env | default |
|---|---|
| `KMINE_BACKEND_URL` | `http://127.0.0.1:8787` |
| `KMINE_BACKEND_TOKEN` | unset (same value as the server if used) |

Path is fixed: `{KMINE_BACKEND_URL}/get_cf_api_key` (no trailing-slash games: trim `/` on the base).

Secret id: `catalog/curseforge`. AES-GCM via existing `Store::put_secret` / `get_secret`. Plaintext JSON:

```json
{ "apiKey": "…", "updatedAt": 1770000000000 }
```

`updatedAt` is unix ms of the last successful write.

HTTP for the key uses existing engine `reqwest` (`HttpFiles` or a one-off GET). Engine still does not depend on `kmine-curseforge`.

Cycle (`catalog/key.rs`), started only from `start_catalog_key_refresh`:

1. Find provider `ProviderId::CURSEFORGE`. Missing → return (no task).
2. Read secret; if `apiKey` present, `set_credentials`.
3. If no secret, fire one GET immediately.
4. `tokio::time::interval(Duration::from_secs(3600))` — also ticks when a secret already exists.
5. `200` + non-empty `apiKey` → `put_secret` then `set_credentials`. Do not delete the old secret until the new write succeeds.
6. Network / 5xx / 401 / decode / empty key → leave secret as-is. Do **not** emit `Event::Error` on a background tick.
7. Opening the catalog: first call `catalog_categories`. `Unavailable` → empty-key panel, do not run search. Do not pretend the search returned zero hits.

`401`/`403` from **Core** after a key was set: surface `CatalogError::Http` on that user action. Do not delete the secret (the hourly job may replace it). Do not touch Microsoft accounts.

The GPUI thread never sees the key string.

## NeoForge and Quilt

```rust
pub enum Loader {
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}
```

`as_str` / serde / SQLite: `neoforge`, `quilt`. `instances.loader` is already `TEXT` — no schema migration. Extend `loader_from_db` and the serde test.

No `PRAGMA user_version` bump.

### Quilt (`crates/engine/src/quilt/`)

Same shape as Fabric:

- Index: `https://meta.quiltmc.org/v3/versions/loader`
- Profile: `https://meta.quiltmc.org/v3/versions/loader/{mc}/{loader}/profile/json`
- `pick_loader_version`: pinned, else first `stable`, else first entry
- `merge_quilt(vanilla, profile)`: replace `main_class`, append libraries, extend arguments

`prepare` runs the Quilt branch the same place as Fabric (before Java/client fetch).

### NeoForge (`crates/engine/src/neoforge/`)

Installer JAR + `install_profile.json` + `version.json` + `forge::run_processors`. Do not copy processors.

- Metadata: `https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml`
- Installer: `https://maven.neoforged.net/releases/net/neoforged/neoforge/{ver}/neoforge-{ver}-installer.jar` and `{url}.sha1`

Version `21.1.66` matches Minecraft `1.21.1`: `1.{major}.{minor}` from the first two dotted components. Treat game id `1.21` as `1.21.0` when comparing. `pick_neoforge_version(mc, versions, preferred)` filters that way; no preferred → highest matching build (same compare style as Forge).

**1.20.1 legacy:** some packs pin `neoforge-47.x.x`. If the chosen version matches `^[0-9]+\.[0-9]+\.[0-9]+$` **and** the first component is `>= 40` (Forge-style), download

`https://maven.neoforged.net/releases/net/neoforged/forge/{ver}/forge-{ver}-installer.jar`

instead of the `neoforge` artifact. `20.2+` / `21.x` always use `neoforge`. Nothing matching → `LoaderUnavailable { loader: NeoForge, minecraft }`.

`prepare`: download installer → `forge::read_installer` → `run_processors` → `merge_forge`. NeoForge installers use the same JSON file names and shape.

### Manifest loader id

`catalog/loader_id.rs`, used by the CF adapter and tests:

| `modLoaders[].id` | Result |
|---|---|
| `forge-47.4.0` | `Forge`, `47.4.0` |
| `fabric-0.16.9` | `Fabric`, `0.16.9` |
| `fabric-0.16.9-1.21.1` | `Fabric`, `0.16.9` when the suffix equals `minecraft.version`; otherwise keep the full remainder after the first `-` |
| `neoforge-21.1.66` | `NeoForge`, `21.1.66` |
| `neoforge-47.1.106` | `NeoForge`, `47.1.106` |
| `quilt-0.27.1` | `Quilt`, `0.27.1` |
| anything else | `UnsupportedLoader { raw }` |

Split on the first `-`. Several `modLoaders`: `primary == true`, else the first. Pin `loader_version` on the instance. Play does not float to latest while the pin is set.

Create-form path still passes `loader_version: None` (latest at prepare), same as today.

## Install pipeline

```rust
impl Engine {
    pub fn add_provider(&self, p: Arc<dyn CatalogProvider>);
    pub fn start_catalog_key_refresh(&self);

    pub async fn catalog_categories(
        &self,
        provider: ProviderId,
        class: ContentClass,
    ) -> Result<Vec<CatalogCategory>, EngineError>;

    pub async fn catalog_search(
        &self,
        query: &CatalogQuery,
    ) -> Result<CatalogPage<CatalogProject>, EngineError>;

    pub async fn catalog_project(
        &self,
        provider: ProviderId,
        id: &CatalogProjectId,
    ) -> Result<CatalogProjectDetail, EngineError>;

    pub async fn catalog_files(
        &self,
        provider: ProviderId,
        id: &CatalogProjectId,
        filter: &CatalogFileFilter,
    ) -> Result<CatalogPage<CatalogFile>, EngineError>;

    pub async fn catalog_file(
        &self,
        provider: ProviderId,
        project_id: &CatalogProjectId,
        file_id: &str,
    ) -> Result<CatalogFile, EngineError>;

    pub async fn cache_remote_image(&self, url: &str) -> Result<PathBuf, EngineError>;

    pub async fn install_pack(
        &self,
        provider: ProviderId,
        project_id: &CatalogProjectId,
        file_id: &str,
        name_override: Option<String>,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
    ) -> Result<InstanceId, EngineError>;

    pub async fn install_content(
        &self,
        instance: InstanceId,
        provider: ProviderId,
        project_id: &CatalogProjectId,
        file_id: &str,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
    ) -> Result<(), EngineError>;
}
```

`cache_remote_image`: GET the URL (no `x-api-key`), write `cache/catalog/images/<sha1(url hex)>` with a guessed extension from `Content-Type` or the URL (`.png` / `.jpg` / `.webp`). Return the path. No eviction in this spec.

Shared file cache for blobs:

```
cache/catalog/files/{provider}/{file_id}/{file_name}
```

If the file exists and advertised sha1 matches (or no sha1 was advertised and size `> 0`), skip the download. After download, if sha1 was advertised and mismatches → `CatalogError::Checksum`, delete the bad file.

One engine-wide install lock (same spirit as `login_lock`). A second `install_pack` / `install_content` while a lock is held, or `install_content` on an instance that is preparing, returns `InstanceBusy`. There is no queue. UI disables Create/Add while any install is running.

### `install_pack`

1. `catalog_file` + `download` the pack zip into the file cache. Progress: `Pack zip`.
2. `parse_pack`. Progress stays on zip until parse returns.
3. `create_instance(CreateInstance { name: name_override.unwrap_or(manifest.name), minecraft_version, loader, loader_version, icon_png })`. `icon_png` is the project logo bytes if `cache_remote_image` + `std::fs::read` succeed; otherwise `None`.
4. For each **required** manifest file: download to cache (progress `Files {done}/{total}` — required files only, not overrides), copy to:
   - `Mods` → `.minecraft/mods/`
   - `ResourcePacks` → `resourcepacks/`
   - `Shaders` → `shaderpacks/`
   - required + no dest → error, rollback
5. `walk_overrides`. Progress `Overrides {done}/{total}` if the zip can count files cheaply; otherwise `Overrides {done}/0` (indeterminate). Write under `instance_minecraft`. Reject if the cleaned path is absolute, contains `..`, or would escape `.minecraft`. Use `/` internally; on Windows convert when creating dirs.
6. Emit `InstancesChanged`. Return the id.

**Rollback:** any error or cancel after step 3 → `delete_instance` (DB row + tree). Leave `cache/catalog/` in place.

UI passes `name_override: None` (manifest name). Rename after, with the existing rename flow.

### `install_content`

1. Load the instance. If `class` of the project (from `catalog_project` / file’s project) is `Mods` and `loader == Vanilla` → `CatalogError::Message("vanilla instance cannot install mods")`.
2. `catalog_file` (caller already picked a file; engine does not re-pick). Download. `resolve_required_deps` with the instance MC version and `Some(loader)` (`None` when Vanilla — only RP/shaders).
3. Write each blob (root + deps) into the dest folder for `CatalogFile.class`. Progress `Content {done}/{total}` including deps.
4. Same filename → overwrite. If `foo.jar.disabled` exists and we install `foo.jar`, overwrite the disabled file and **leave it disabled** (write to `foo.jar.disabled`).
5. Write via `{name}.part` in the dest dir + `rename`. Never leave a `.part` after return (delete on error).

**Rollback:** do not delete jars that already landed. Stop the batch. Instance remains.

### Path copy helper

Copy from cache to dest with `std::fs::copy`. Create the dest folder. Do not follow dest symlinks out of the instance (write using the instance dir as root; if `dest` after `canonicalize` of the parent is outside `instance_minecraft`, error).

## UI

Existing 440px `sheet` stays for the loader form. Catalog is a **wider** sheet on the same dimmer, not a new window and not a main-window tab.

### Create — kind grid

`CreatePhase::Kind` first screen, 440px, 2×3 buttons:

| Vanilla | Fabric | Forge |
| NeoForge | Quilt | Modpack |

Each cell: existing cover (`assets/icons/covers/{loader}.jpg`) + label. Add `neoforge.jpg` and `quilt.jpg`. Until original art exists, **copy** `forge.jpg` → `neoforge.jpg` and `fabric.jpg` → `quilt.jpg` so the grid is not empty. Modpack uses `IconName` (box/store), not a loader cover.

- Five loaders → current form: name, Minecraft version, **Create**. Remove the 3-way loader segmented control; loader is the kind. `loader_version` stays `None`. **Back** returns to the grid.
- **Modpack** closes the 440px sheet and opens the catalog with `ContentClass::Modpacks`. Dismissing the catalog ends create (does not return to the grid). `+` opens the grid again.
- Opening Accounts or Settings still cancels create (current behavior).

### Catalog modal (880px, max ~80vh)

One component, two entry points:

| Entry | `class` | Success |
|---|---|---|
| Create → Modpack | `Modpacks` | `install_pack`, close, `selected = new id` |
| Content → Add on a folder | that folder’s class | `install_content`, close, `reload_content` |

Vanilla + Mods: no Add button. Vanilla + resource packs / shaders: Add is shown.

Header: provider chip (only `CurseForge` for now, selected). Later a second chip; queries stay single-provider.

**List**

- Search input, 300ms debounce, cancel the in-flight `catalog_search` when the query changes
- Category chips from `catalog_categories` (multi-select, max 10, AND)
- Sort: Popularity / Updated / Downloads / Name
- Add flow: `game_version` and `loader` **fixed** from the instance, not editable
- Pack flow: optional MC version filter; empty = all versions
- Card: logo (`cache_remote_image`), name, authors, download count
- Footer **More** uses `index + page_size` while `index + page_size < total` (and respects the 10000 cap). No implicit infinite scroll

**Project**

- Back to list
- Name, summary, screenshots via `cache_remote_image`
- Do **not** render `description_html`. If `website_url` is set, show it as a text link (open in OS browser)
- File list from `catalog_files` with the same version/loader filter as the list
- Row: display name, release, game versions, date
- Selecting a row sets the install target. If the first page has exactly one file, it is preselected
- Primary **Install** disabled until a file is selected. Pack zip is parsed only after click; `UnsupportedLoader` / `Manifest` then fail progress and roll back (no instance)

**No key**

Not an empty list. Title + copy: the backend `GET /get_cf_api_key` has never returned a key. Search and categories disabled.

Search/project HTTP errors: `Alert` in the body; keep the last good list.

### Install progress

Close the catalog (and create overlay). Reuse the existing bottom `ProgressModal` / `EventProgressSink` with titles from `ProgressSink` (`Pack zip`, `Mods 3/140`, …). Cancel uses the same `CancellationToken` as prepare.

While an install is running, Create and Add do not open.

Pack failure: no instance (rollback), `status` = error text. Add failure: instance stays, reload content.

### Content tab

`section_header` grows an **Add** action on the right, except vanilla Mods. Toggle/delete unchanged.

### `KmineApp` state

Do not hang pagination off `CreateInstanceForm`.

```rust
enum CreatePhase { Kind, Loader(Loader) }

struct CatalogModal {
    provider: ProviderId,
    class: ContentClass,
    target: CatalogTarget, // NewInstance | Instance(InstanceId)
    search: Entity<InputState>,
    categories: Vec<CatalogCategory>,
    selected_categories: Vec<String>,
    sort: CatalogSort,
    page: Option<CatalogPage<CatalogProject>>,
    project: Option<CatalogProjectDetail>,
    files: Option<CatalogPage<CatalogFile>>,
    selected_file: Option<String>,
    error: Option<String>,
    loading: bool,
}
```

Hourly key state is not mirrored in the UI.

`chrome.rs`: `loader_label` / `loader_icon` / covers gain NeoForge and Quilt. Instance settings loader dropdown includes both.

## Errors (UI mapping)

| Error | UI |
|---|---|
| `CatalogError::Unavailable` | Catalog empty-key panel |
| `NotFound` | Alert; list unchanged |
| `UnknownProvider` | Alert (should not happen with one chip) |
| `Http` | Alert. `url` has no credentials |
| `UnsupportedLoader` | After pack parse: progress error, rollback. Create grid still offers only supported loaders |
| `Manifest` / `Checksum` | Pack rollback or Add stop |
| `EngineError::Cancelled` | Dismiss progress; pack rolled back |
| `InstanceBusy` | Ignore the second click (button disabled) |

Background key refresh never sends `Event::Error`. User-initiated catalog calls do.

## Tests

Default `cargo test` hits no live `api.curseforge.com`, no live maven/meta, no real `backend-api`. Wiremock + fixtures only.

### `backend-api`

- Fixture tree whose bytes contain a bcrypt-shaped key matching the extractor regex → `GET` 200, JSON `apiKey` equals the fixture, `source` non-empty
- Missing / empty `KMINE_CF_KEY_SOURCE` → process can start but GET is 503, body has no `$2a$10$` key
- `KMINE_BACKEND_TOKEN=secret` without header → 401
- 401/503 bodies do not contain the fixture key

### `kmine-engine`

- `parse_manifest_loader` table from the manifest section
- Quilt profile fixture → `merge_quilt` changes `main_class` and appends libraries
- `pick_neoforge_version("1.21.1", ["21.1.1", "21.0.1", "20.4.1"], None)` → `21.1.1`
- `pick_neoforge_version` + preferred `47.1.106` selects that string (legacy URL is a unit on the URL helper, not a live GET)
- Key: wiremock 200 → secret written, CF-less fake provider `has_credentials`; next GET 503 → secret unchanged; `EngineError` / `CatalogError` `to_string()` does not contain the fixture key
- Pack zip fixture: `manifest.json` + one required file id (mocked download) + `overrides/config/a.toml` → instance exists, loader/version set, jar in `mods/`, override at `.minecraft/config/a.toml`
- Cancel after `create_instance` inside `install_pack` → no instance row, no instance dir
- Override `../escape` → error, no file outside `.minecraft`
- `install_content` Mods on Vanilla → error
- `install_content` + one required dep → two files under `mods/`
- `install_pack` / `install_content` while preparing → `InstanceBusy`
- `loader_from_db("neoforge"|"quilt")` ok; unknown string still fails

### `kmine` adapter

Unit tests (no HTTP): JEI fixture `Mod` → `CatalogProject` class `Mods`; `ModLoaderType::Forge` → `Loader::Forge`; `ClassId::MODPACKS` → `supports(Modpacks)`; worlds class → `supports` false.

No GPUI tests.

`kmine-curseforge` tests are unchanged by this spec.

## Security

- Core key at rest: existing AES-GCM secrets, AAD = `catalog/curseforge`
- Core key in transit from `backend-api`: HTTP. Default bind is loopback. Optional Bearer. Not TLS in this spec
- Key never in UI state, logs, `Debug` of `Client`/`CurseForgeProvider`/`Engine`, or `Event` payloads
- Override zip-slip rejected
- `allow_mod_distribution == false` is not a hard stop (crate policy); 403 is `Http`
- `backend-api` is a privileged extractor. Do not ship a baked key in `kmine`

## Alternatives (rejected)

1. **Engine depends on `kmine-curseforge` directly** — faster, but every provider becomes an engine dep and CF types leak upward.
2. **Catalog in the GPUI crate, engine only `write_bytes`** — breaks “UI does not know Mojang/Forge/CF” and duplicates install/cancel.
3. **`backend-api` proxies the whole catalog** — contradicts “the handle returns the key; the client talks to Core”.
4. **Env-only or settings-pasted key, no backend** — rejected; key comes from `/get_cf_api_key`.
5. **Install from the search card without a project page** — rejected; packs with several MC lines need a file pick.
6. **Skip NeoForge/Quilt** — rejected; those packs must launch.

## Risks

| Risk | Severity | Mitigation |
|---|---|---|
| NeoForge 1.20.1 artifact split | High | Explicit `47.x` → `net.neoforged:forge` installer URL |
| SkyFactory-sized override trees | Med | Stream `walk_overrides`; do not buffer all overrides |
| `backend-api` down after first success | Low | Hourly fail keeps last secret |
| `backend-api` never up | Med | Catalog empty-key state, rest of launcher works |
| 880px modal on a small laptop | Low | Sheet max width `min(880, window-32)` |
| Description HTML unsafe | Low | Do not render it |
| Two installs at once | Low | Single engine install lock |

## Implementation slices

Each slice leaves `cargo test -p` of the touched package green. Order is the implementation plan.

1. **Loaders** — `Loader` + Quilt prepare + NeoForge prepare + chrome labels/covers + create kind grid with the five loaders only (no Modpack cell yet)
2. **`backend-api`** — extract cache + GET + tests
3. **Engine key + trait** — types, `CatalogProvider`, fake provider in tests, hourly refresh against wiremock, `add_provider`
4. **CF adapter + install** — `src/providers/curseforge.rs`, `install_pack` / `install_content`, cache, rollback tests
5. **Catalog UI** — modal list/project/files, kind → Modpack, Content Add, progress reuse

## Key decisions

1. **Trait in engine, adapter in the binary** — second provider does not touch install or GPUI beyond a chip.
2. **Key lives in `backend-api`, hourly secret in SQLite** — launcher never parses Overwolf sources; catalog survives backend blips after the first success.
3. **One catalog modal, two targets** — create-from-pack vs add-to-instance share search/project/file.
4. **File pick before install** — required for multi-version packs; optional deps deferred.
5. **NeoForge and Quilt in this spec** — otherwise most current CF packs cannot launch.
6. **Pack is transactional; Add is not** — a half-installed pack is worse than a half-added mod list.
7. **No provenance table** — YAGNI until updates exist.
8. **No HTML description** — avoid a webview; summary + website link is enough.

## Relationship to other specs

- 2026-08-14 launcher: strike “no in-app store / no modpack installer / no NeoForge” for the items above. Auth, sandbox, prepare-for-vanilla/Fabric/Forge, content enable/disable remain as written.
- 2026-08-15 curseforge crate: operator recipe (download zip → parse → resolve → write) is now this spec’s `install_pack`. The crate still does not write paths.

## Open questions

None. Choices that were open in conversation are locked in **Decisions**.
