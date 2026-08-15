# Catalog Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship NeoForge/Quilt prepare, a separate `kmine-backend-api` key process, engine catalog install, a CurseForge adapter, and the create/Add catalog modal — without the launcher extracting the Core key.

**Architecture:** `kmine-engine` owns `Loader`, `CatalogProvider`, hourly key refresh, and disk install. `kmine` injects `CurseForgeProvider` (wraps `kmine-curseforge::Client`) and draws the wizard/catalog. `crates/backend-api` (`kmine-backend-api`) is a separate process: `extract_from_source` + `GET /get_cf_api_key`.

**Tech Stack:** Rust edition 2024, existing `reqwest`/`rusqlite`/`tokio`/`thiserror`/`gpui`, new `async_trait` on engine, `axum` on `kmine-backend-api`. Tests: wiremock + fixtures. No live Core/maven/meta/`backend-api` in default `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-15-catalog-integration-design.md`

## Global Constraints

- Rust edition **2024**.
- `kmine-engine` must **not** depend on `kmine-curseforge`.
- `kmine-backend-api` uses `extract_*` only — no catalog `Client`.
- UI never imports CF types (`Mod`, `ClassId`, `SearchQuery`).
- Default `cargo test` hits no `api.curseforge.com`, no live NeoForge/Quilt maven/meta, no real backend.
- Core key never appears in `Display`/`Debug`/`Event`/logs/error bodies.
- One `provider` per catalog query. No merged feed.
- Optional deps, provenance/updates, worlds/datapacks, Modrinth implementation: out.
- After every task the touched package’s tests stay green (`cargo test -p kmine-engine`, `cargo test -p kmine-backend-api`, `cargo test -p kmine` as applicable).
- `cargo check -p kmine` after any `Loader` match change (chrome/create are exhaustive).

## File structure

| Path | Responsibility |
|---|---|
| `crates/engine/src/ids.rs` | `Loader::{NeoForge, Quilt}` |
| `crates/engine/src/store/mod.rs` | `loader_from_db` arms |
| `crates/engine/src/error.rs` | `EngineError::Catalog` |
| `crates/engine/src/quilt/mod.rs` | Quilt meta + merge |
| `crates/engine/src/neoforge/mod.rs` | version pick + installer URL + prepare |
| `crates/engine/src/launch/mod.rs` | prepare branches |
| `crates/engine/src/lib.rs` | modules, `add_provider`, re-exports |
| `crates/engine/src/catalog/mod.rs` | re-exports |
| `crates/engine/src/catalog/types.rs` | `ContentClass`, `Catalog*`, `CatalogError` |
| `crates/engine/src/catalog/provider.rs` | `CatalogProvider`, `ProviderId` |
| `crates/engine/src/catalog/loader_id.rs` | `parse_manifest_loader` |
| `crates/engine/src/catalog/query.rs` | `catalog_*` dispatch |
| `crates/engine/src/catalog/key.rs` | secret + refresh |
| `crates/engine/src/catalog/cache.rs` | file cache + `cache_remote_image` |
| `crates/engine/src/catalog/install.rs` | `install_pack` / `install_content` |
| `crates/engine/src/paths.rs` | `cache_catalog` dirs |
| `crates/backend-api/` | `kmine-backend-api` binary |
| `src/providers/curseforge.rs` | CF adapter |
| `src/modals/create_instance.rs` | kind grid + loader form |
| `src/modals/catalog.rs` | list / project / files |
| `src/modals/mod.rs` | `pub mod catalog` |
| `src/screens/instance_content.rs` | Add |
| `src/chrome.rs` / `src/assets.rs` | labels, covers |
| `src/app.rs` / `src/main.rs` | wire providers, modal state |
| `assets/icons/covers/neoforge.jpg` | copy of `forge.jpg` |
| `assets/icons/covers/quilt.jpg` | copy of `fabric.jpg` |

---

### Task 1: Loader enum, SQLite, chrome covers

**Files:**
- Modify: `crates/engine/src/ids.rs`
- Modify: `crates/engine/src/store/mod.rs` (`loader_from_db`)
- Modify: `src/chrome.rs` (`loader_icon`, `default_cover`, `loader_label`)
- Modify: `src/assets.rs`
- Create: `assets/icons/covers/neoforge.jpg` (copy `forge.jpg`)
- Create: `assets/icons/covers/quilt.jpg` (copy `fabric.jpg`)

**Interfaces:**
- Consumes: existing `Loader::{Vanilla, Fabric, Forge}`
- Produces: `Loader::{NeoForge, Quilt}`; `as_str` → `"neoforge"` / `"quilt"`; serde lowercase; `loader_from_db` accepts those strings

- [ ] **Step 1: Extend the serde test so it fails to compile / assert the new variants**

In `crates/engine/src/ids.rs` tests, add:

```rust
assert_eq!(
    serde_json::from_str::<Loader>("\"neoforge\"").unwrap(),
    Loader::NeoForge
);
assert_eq!(
    serde_json::from_str::<Loader>("\"quilt\"").unwrap(),
    Loader::Quilt
);
assert_eq!(serde_json::to_string(&Loader::NeoForge).unwrap(), "\"neoforge\"");
assert_eq!(serde_json::to_string(&Loader::Quilt).unwrap(), "\"quilt\"");
```

- [ ] **Step 2: Run the test — it must not compile (variants missing)**

Run: `cargo test -p kmine-engine ids::tests -- --nocapture`

Expected: compile error `no variant named NeoForge`

- [ ] **Step 3: Add variants and wire persistence + chrome**

```rust
pub enum Loader {
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}

impl Loader {
    pub fn as_str(self) -> &'static str {
        match self {
            Loader::Vanilla => "vanilla",
            Loader::Fabric => "fabric",
            Loader::Forge => "forge",
            Loader::NeoForge => "neoforge",
            Loader::Quilt => "quilt",
        }
    }
}
```

`loader_from_db`:

```rust
"neoforge" => Ok(Loader::NeoForge),
"quilt" => Ok(Loader::Quilt),
```

`loader_icon`: NeoForge → `IconName::Cpu`, Quilt → `IconName::Frame`.

`default_cover` / `loader_label`: `"icons/covers/neoforge.jpg"` / `"NeoForge"`, `"icons/covers/quilt.jpg"` / `"Quilt"`.

```bash
cp assets/icons/covers/forge.jpg assets/icons/covers/neoforge.jpg
cp assets/icons/covers/fabric.jpg assets/icons/covers/quilt.jpg
```

Register both in `src/assets.rs` `CUSTOM` the same way as `forge.jpg`.

There is **no** loader dropdown in `instance_settings.rs`. Do not add one.

- [ ] **Step 4: Run tests and check the UI crate**

Run: `cargo test -p kmine-engine ids::tests store::`
Run: `cargo check -p kmine`

Expected: PASS / check ok. Every `match loader` in the repo must be exhaustive.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/ids.rs crates/engine/src/store/mod.rs \
  src/chrome.rs src/assets.rs \
  assets/icons/covers/neoforge.jpg assets/icons/covers/quilt.jpg
git commit -m "feat(engine): add NeoForge and Quilt loader variants"
```

---

### Task 2: Quilt prepare

**Files:**
- Create: `crates/engine/src/quilt/mod.rs`
- Create: `crates/engine/tests/fixtures/quilt_loader.json`
- Create: `crates/engine/tests/fixtures/quilt_profile.json`
- Modify: `crates/engine/src/lib.rs` (`pub mod quilt;`)
- Modify: `crates/engine/src/launch/mod.rs` (Quilt branch next to Fabric)

**Interfaces:**
- Consumes: `VersionInfo`, `Loader::Quilt`, `HttpFiles::get_json`
- Produces:
  - `pub const LOADER_INDEX_URL: &str = "https://meta.quiltmc.org/v3/versions/loader";`
  - `pub fn profile_url(mc: &str, loader: &str) -> String`
  - `pub fn pick_loader_version(index: &QuiltLoaderIndex, preferred: Option<&str>) -> Result<String, EngineError>`
  - `pub fn merge_quilt(vanilla: VersionInfo, profile: QuiltProfile) -> VersionInfo`
  - `prepare` for `Loader::Quilt` calls merge before Java/client (same slot as Fabric)

- [ ] **Step 1: Write fixtures and failing tests in `quilt/mod.rs`**

`quilt_loader.json`:

```json
[{"version":"0.27.1","stable":true},{"version":"0.26.0","stable":false}]
```

`quilt_profile.json`:

```json
{
  "mainClass": "org.quiltmc.loader.impl.launch.knot.KnotClient",
  "arguments": {"game": ["--quiltGame"], "jvm": ["-DQuiltMcEmu=net.minecraft.client.main.Main"]},
  "libraries": [{"name": "org.quiltmc:quilt-loader:0.27.1", "url": "https://maven.quiltmc.org/repository/release/"}]
}
```

Tests (mirror `fabric/mod.rs`):

```rust
#[test]
fn pick_stable_loader() {
    let idx = load_loader_index();
    assert_eq!(pick_loader_version(&idx, None).unwrap(), "0.27.1");
    assert_eq!(pick_loader_version(&idx, Some("0.26.0")).unwrap(), "0.26.0");
}

#[test]
fn profile_url_uses_meta_quilt_v3() {
    assert_eq!(
        profile_url("1.21.1", "0.27.1"),
        "https://meta.quiltmc.org/v3/versions/loader/1.21.1/0.27.1/profile/json"
    );
}

#[test]
fn merge_replaces_main_class_and_adds_lib() {
    let v = merge_quilt(load_version("version_1_21.json"), load_profile());
    assert_eq!(v.main_class, "org.quiltmc.loader.impl.launch.knot.KnotClient");
    assert!(v.libraries.iter().any(|l| l.name.contains("quilt-loader")));
}

#[test]
fn pick_empty_index_without_preferred_is_unavailable() {
    let err = pick_loader_version(&QuiltLoaderIndex(Vec::new()), None).unwrap_err();
    assert!(matches!(err, EngineError::LoaderUnavailable { loader: Loader::Quilt, .. }));
}
```

- [ ] **Step 2: Run tests — fail (module missing)**

Run: `cargo test -p kmine-engine quilt:: -- --nocapture`

Expected: fail / compile error

- [ ] **Step 3: Implement Quilt like Fabric**

Copy `crates/engine/src/fabric/mod.rs` structure. Types: `QuiltLoaderIndex(pub Vec<QuiltLoaderEntry>)`, `QuiltLoaderEntry { version, stable, .. }`, `QuiltProfile` / `QuiltLibrary` same JSON shape as Fabric (`mainClass`, `libraries`, `arguments`). `pick_loader_version` uses `Loader::Quilt` in `LoaderUnavailable`. `library_from_quilt` is the same maven-path helper as Fabric (copy the private functions into this module; do not refactor Fabric in this task).

In `launch/mod.rs`, change the Fabric-only merge to:

```rust
let mut version = match row.loader {
    Loader::Fabric => merge_fabric_profile(&http, &row, version, progress, cancel).await?,
    Loader::Quilt => merge_quilt_profile(&http, &row, version, progress, cancel).await?,
    _ => version,
};
```

`merge_quilt_profile` is a copy of `merge_fabric_profile` using `quilt::{LOADER_INDEX_URL, pick_loader_version, profile_url, merge_quilt, QuiltLoaderIndex, QuiltProfile}` and progress title `"Quilt loader"`. 404 → `LoaderUnavailable { Quilt, mc }`.

Add `prepare_quilt_offline_errors_without_account` next to the Fabric one in `launch/mod.rs` tests (same body, `Loader::Quilt`; must not be `LoaderUnavailable`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine quilt:: launch::tests::prepare_quilt`

Expected: PASS. Offline prepare may still hit network for the version manifest and then fail `NoAccount` — same as existing Fabric test.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/quilt crates/engine/src/lib.rs crates/engine/src/launch/mod.rs \
  crates/engine/tests/fixtures/quilt_loader.json crates/engine/tests/fixtures/quilt_profile.json
git commit -m "feat(engine): prepare Quilt like Fabric"
```

---

### Task 3: NeoForge prepare

**Files:**
- Create: `crates/engine/src/neoforge/mod.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod neoforge;`)
- Modify: `crates/engine/src/launch/mod.rs` (NeoForge uses installer + processors)

**Interfaces:**
- Consumes: `forge::{read_installer, run_processors, merge_forge, ForgeInstallProfile}`, `HttpFiles`
- Produces:
  - `pub const MAVEN_METADATA_URL: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";`
  - `pub fn installer_url(ver: &str) -> String`
  - `pub fn is_legacy_forge_artifact(ver: &str) -> bool`
  - `pub fn minecraft_from_neoforge(ver: &str) -> Option<String>`
  - `pub fn normalize_minecraft(mc: &str) -> String` — `"1.21"` → `"1.21.0"`
  - `pub fn pick_neoforge_version(mc: &str, versions: &[String], preferred: Option<&str>) -> Result<String, EngineError>`
  - `pub async fn prepare_neoforge(...) -> Result<(ForgeInstallProfile, VersionInfo), EngineError>`

- [ ] **Step 1: Write failing URL / pick tests**

```rust
#[test]
fn installer_url_modern() {
    assert_eq!(
        installer_url("21.1.66"),
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.66/neoforge-21.1.66-installer.jar"
    );
}

#[test]
fn installer_url_legacy_47() {
    assert_eq!(
        installer_url("47.1.106"),
        "https://maven.neoforged.net/releases/net/neoforged/forge/47.1.106/forge-47.1.106-installer.jar"
    );
}

#[test]
fn pick_filters_by_minecraft() {
    let versions = ["21.1.1", "21.0.1", "20.4.1"].map(String::from);
    assert_eq!(
        pick_neoforge_version("1.21.1", &versions, None).unwrap(),
        "21.1.1"
    );
}

#[test]
fn pick_treats_1_21_as_1_21_0() {
    let versions = ["21.0.3", "21.1.1"].map(String::from);
    assert_eq!(
        pick_neoforge_version("1.21", &versions, None).unwrap(),
        "21.0.3"
    );
}

#[test]
fn pick_preferred_wins_even_if_legacy() {
    let versions = ["21.1.1"].map(String::from);
    assert_eq!(
        pick_neoforge_version("1.21.1", &versions, Some("47.1.106")).unwrap(),
        "47.1.106"
    );
}

#[test]
fn pick_empty_is_unavailable() {
    let err = pick_neoforge_version("1.21.1", &[], None).unwrap_err();
    assert!(matches!(err, EngineError::LoaderUnavailable { loader: Loader::NeoForge, .. }));
}
```

- [ ] **Step 2: Run — fail**

Run: `cargo test -p kmine-engine neoforge:: -- --nocapture`

Expected: compile error

- [ ] **Step 3: Implement**

```rust
pub fn is_legacy_forge_artifact(ver: &str) -> bool {
    ver.split('.').next()
        .and_then(|s| s.parse::<u32>().ok())
        .is_some_and(|n| n >= 40)
}

pub fn minecraft_from_neoforge(ver: &str) -> Option<String> {
    if is_legacy_forge_artifact(ver) {
        return None;
    }
    let mut parts = ver.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    Some(format!("1.{major}.{minor}"))
}

pub fn normalize_minecraft(mc: &str) -> String {
    let dots = mc.bytes().filter(|&b| b == b'.').count();
    if dots == 1 { format!("{mc}.0") } else { mc.to_string() }
}

pub fn pick_neoforge_version(
    mc: &str,
    versions: &[String],
    preferred: Option<&str>,
) -> Result<String, EngineError> {
    if let Some(preferred) = preferred {
        return Ok(preferred.to_string());
    }
    let want = normalize_minecraft(mc);
    versions
        .iter()
        .filter(|v| minecraft_from_neoforge(v).as_deref() == Some(want.as_str()))
        .max_by(|a, b| crate::forge::cmp_forge_version(a, b))
        .cloned()
        .ok_or_else(|| EngineError::LoaderUnavailable {
            loader: Loader::NeoForge,
            minecraft: mc.to_string(),
        })
}

pub fn installer_url(ver: &str) -> String {
    if is_legacy_forge_artifact(ver) {
        format!(
            "https://maven.neoforged.net/releases/net/neoforged/forge/{ver}/forge-{ver}-installer.jar"
        )
    } else {
        format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{ver}/neoforge-{ver}-installer.jar"
        )
    }
}
```

Change `fn cmp_forge_version` in `crates/engine/src/forge/mod.rs` (line 353) to `pub(crate) fn cmp_forge_version`. Do not copy it.

`prepare_neoforge`: copy `prepare_forge` but:

- metadata URL = NeoForge maven
- cache file `neoforge-maven-metadata.xml`
- `pick_neoforge_version`
- `installer_url(&ver)`
- dest `cache/libraries/net/neoforged/{neoforge|forge}/{ver}/{artifact}-{ver}-installer.jar`
- `LoaderUnavailable` uses `Loader::NeoForge`
- `read_installer` + return `(profile, version)` — `launch` runs processors + `merge_forge`

In `launch/mod.rs`:

```rust
let installer = match row.loader {
    Loader::Forge => Some(prepare_forge(...).await?),
    Loader::NeoForge => Some(prepare_neoforge(...).await?),
    _ => None,
};
// existing: if let Some((profile, forge_version)) = installer { run_processors; merge_forge }
```

Add `prepare_neoforge_offline_errors_without_account` like Forge.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine neoforge:: launch::tests::prepare_neoforge`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/neoforge crates/engine/src/lib.rs crates/engine/src/launch/mod.rs \
  crates/engine/src/forge/mod.rs
git commit -m "feat(engine): prepare NeoForge via installer processors"
```

---

### Task 4: Create-instance kind grid

**Files:**
- Modify: `src/modals/create_instance.rs`
- Modify: `src/app.rs` (`open_create`, `set_create_loader` → kind/phase)

**Interfaces:**
- Consumes: `Loader` five variants, `default_cover`, `loader_label`
- Produces: `CreatePhase::{Kind, Loader(Loader)}` on the form; Modpack cell **not** in this task

- [ ] **Step 1: No GPUI test (spec). Write the form state so `spec()` still returns `CreateInstance`**

Replace `CreateInstanceForm.loader: Loader` usage as the first screen.

```rust
pub enum CreatePhase {
    Kind,
    Loader(Loader),
}

pub struct CreateInstanceForm {
    pub name: Entity<InputState>,
    pub version: Entity<InputState>,
    pub phase: CreatePhase,
}

impl CreateInstanceForm {
    pub fn spec(&self, cx: &App) -> Option<CreateInstance> {
        let CreatePhase::Loader(loader) = self.phase else {
            return None;
        };
        Some(CreateInstance {
            name: self.name.read(cx).value().to_string(),
            minecraft_version: self.version.read(cx).value().to_string(),
            loader,
            loader_version: None,
            icon_png: None,
        })
    }
}
```

`render`: if `Kind`, 2-row wrap of five buttons (Vanilla, Fabric, Forge, NeoForge, Quilt) — cover image + label. Click → `on_kind(Loader)`. If `Loader(l)`, current name/version fields + **Back** (`on_back`) + **Create**. Delete the 3-way `segmented` loader picker.

`open_create` sets `phase: Kind`. `submit_create` no-ops if `spec()` is `None`.

- [ ] **Step 2: `cargo check -p kmine`**

Expected: ok. Manual: `+` shows five kinds; pick Fabric → form; Back → grid; Create still inserts an instance.

- [ ] **Step 3: Commit**

```bash
git add src/modals/create_instance.rs src/app.rs
git commit -m "feat(ui): create-instance kind grid for five loaders"
```

---

### Task 5: `kmine-backend-api`

**Files:**
- Modify: `Cargo.toml` workspace `members`
- Create: `crates/backend-api/Cargo.toml`
- Create: `crates/backend-api/src/main.rs`
- Create: `crates/backend-api/tests/get_cf_api_key.rs`
- Create: `crates/backend-api/tests/fixtures/key.txt` — one line, the sample key from `kmine-curseforge` extract tests: `$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm`

**Interfaces:**
- Consumes: `kmine_curseforge::extract_from_source`
- Produces: process listening on `KMINE_BACKEND_BIND` (default `127.0.0.1:8787`); `GET /get_cf_api_key` → `{ "apiKey": "...", "source": "..." }`

- [ ] **Step 1: Write the integration test first (it will fail: crate missing)**

`crates/backend-api/tests/get_cf_api_key.rs` — spawn the binary via a helper later; for a unit-shaped test, put the handler in `lib.rs` so tests do not need a child process:

Also create `crates/backend-api/src/lib.rs` with `pub fn app(state: AppState) -> Router` so tests use `oneshot`.

```rust
#[tokio::test]
async fn returns_key_from_source_file() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/key.txt");
    let state = AppState::from_source(fixture.to_str().unwrap()).unwrap();
    let app = app(state);
    let resp = app
        .oneshot(Request::get("/get_cf_api_key").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        body["apiKey"],
        "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm"
    );
    assert!(body["source"].as_str().unwrap().len() > 0);
}

#[tokio::test]
async fn missing_source_is_503_without_key() {
    let state = AppState::empty();
    let app = app(state);
    let resp = app.oneshot(Request::get("/get_cf_api_key").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("$2a$10$"));
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"], "key unavailable");
}

#[tokio::test]
async fn token_required_yields_401() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/key.txt");
    let mut state = AppState::from_source(fixture.to_str().unwrap()).unwrap();
    state.token = Some("secret".into());
    let app = app(state);
    let resp = app.oneshot(Request::get("/get_cf_api_key").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let text = String::from_utf8_lossy(&resp.into_body().collect().await.unwrap().to_bytes());
    assert!(!text.contains("$2a$10$"));
}
```

`Cargo.toml` package name: `kmine-backend-api`. Deps: `axum` (json), `tokio` (macros, rt-multi-thread, signal), `serde`/`serde_json`, `kmine-curseforge = { path = "../curseforge" }`. Dev: `http-body-util`, `tower` (util) for `oneshot`.

Workspace members: `".", "crates/engine", "crates/curseforge", "crates/backend-api"`.

- [ ] **Step 2: Run — fail**

Run: `cargo test -p kmine-backend-api -- --nocapture`

Expected: package or test compile fail

- [ ] **Step 3: Implement**

```rust
pub struct AppState {
    pub key: Option<CfCoreKey>,
    pub token: Option<String>,
    pub source: Option<String>,
    pub last_mtime: Option<std::time::SystemTime>,
    pub last_url_extract: Option<std::time::Instant>,
}

impl AppState {
    pub fn empty() -> Self { /* all None */ }

    pub fn from_source(source: &str) -> Result<Self, String> {
        let key = extract_from_source(source).ok();
        Ok(Self {
            key,
            token: std::env::var("KMINE_BACKEND_TOKEN").ok().filter(|s| !s.is_empty()),
            source: Some(source.to_string()),
            last_mtime: mtime_if_path(source),
            last_url_extract: source.starts_with("http").then(std::time::Instant::now),
        })
    }
}

async fn get_cf_api_key(State(st): State<Arc<RwLock<AppState>>>) -> impl IntoResponse {
    refresh_if_needed(&st);
    let st = st.read().unwrap();
    let Some(key) = st.key.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"key unavailable"}))).into_response();
    };
    (
        StatusCode::OK,
        Json(json!({"apiKey": key.key, "source": key.source})),
    ).into_response()
}
```

Middleware: if `state.token` is `Some`, require `Authorization: Bearer <token>` else 401 `{"error":"unauthorized"}`.

`refresh_if_needed`: if source is a local path and `mtime` changed, `extract_from_source` again; if source is URL and `last_url_extract` older than 24h, extract again. Never log `key.key`.

`main.rs`: bind `KMINE_BACKEND_BIND` default `127.0.0.1:8787`; `KMINE_CF_KEY_SOURCE` required for a useful server (process still starts if missing — state empty → 503). Handle `SIGHUP` (`tokio::signal::unix` on unix; no-op on Windows) by re-extracting.

Do not print the key on stdout.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-backend-api`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/backend-api
git commit -m "feat(backend-api): serve extracted CurseForge Core key"
```

---

### Task 6: Catalog types, errors, `parse_manifest_loader`

**Files:**
- Modify: `crates/engine/Cargo.toml` — add `async-trait = "0.1"`
- Create: `crates/engine/src/catalog/mod.rs`
- Create: `crates/engine/src/catalog/types.rs`
- Create: `crates/engine/src/catalog/provider.rs` (`ProviderId` only + empty trait stub if needed next task)
- Create: `crates/engine/src/catalog/loader_id.rs`
- Modify: `crates/engine/src/error.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod catalog;` re-exports)

**Interfaces:**
- Consumes: `Loader`, `ContentFolder`
- Produces: types exactly as the spec (`ContentClass`, `ProviderId`, `CatalogProjectId`, `CatalogCredentials`, `CatalogSort`, `CatalogRelease`, `CatalogQuery`, `CatalogCategory`, `CatalogProject`, `CatalogProjectDetail`, `CatalogFile` **with `class`**, `CatalogFileFilter`, `CatalogBlob`, `CatalogPage<T>`, `PackManifestSpec`, `PackManifestFileSpec`, `PackOverride`, `CatalogError`, `CatalogResource`)
- Produces: `pub fn parse_manifest_loader(id: &str, minecraft_version: &str) -> Result<(Loader, String), CatalogError>`
- Produces: `EngineError::Catalog(CatalogError)`

- [ ] **Step 1: Write `parse_manifest_loader` table test**

```rust
#[test]
fn parse_manifest_loader_table() {
    let cases = [
        ("forge-47.4.0", "1.20.1", Loader::Forge, "47.4.0"),
        ("fabric-0.16.9", "1.21.1", Loader::Fabric, "0.16.9"),
        ("fabric-0.16.9-1.21.1", "1.21.1", Loader::Fabric, "0.16.9"),
        ("fabric-0.16.9-1.21.1", "1.20.1", Loader::Fabric, "0.16.9-1.21.1"),
        ("neoforge-21.1.66", "1.21.1", Loader::NeoForge, "21.1.66"),
        ("neoforge-47.1.106", "1.20.1", Loader::NeoForge, "47.1.106"),
        ("quilt-0.27.1", "1.21.1", Loader::Quilt, "0.27.1"),
    ];
    for (id, mc, loader, ver) in cases {
        assert_eq!(parse_manifest_loader(id, mc).unwrap(), (loader, ver.to_string()), "{id}");
    }
    assert!(matches!(
        parse_manifest_loader("liteloader-1.12", "1.12"),
        Err(CatalogError::UnsupportedLoader { .. })
    ));
}

#[test]
fn catalog_error_display_has_no_key_looking_secret() {
    let err = CatalogError::Http { url: "http://127.0.0.1:8787/get_cf_api_key".into(), status: 503 };
    let s = err.to_string();
    assert!(!s.contains("apiKey"));
    assert!(!s.contains("$2a$10$"));
}
```

- [ ] **Step 2: Run — fail**

Run: `cargo test -p kmine-engine catalog:: -- --nocapture`

Expected: compile error

- [ ] **Step 3: Implement types + parser + error**

`parse_manifest_loader`:

```rust
pub fn parse_manifest_loader(id: &str, minecraft_version: &str) -> Result<(Loader, String), CatalogError> {
    let (prefix, rest) = id.split_once('-').ok_or_else(|| CatalogError::UnsupportedLoader { raw: id.into() })?;
    let loader = match prefix {
        "forge" => Loader::Forge,
        "fabric" => Loader::Fabric,
        "neoforge" => Loader::NeoForge,
        "quilt" => Loader::Quilt,
        _ => return Err(CatalogError::UnsupportedLoader { raw: id.into() }),
    };
    let version = match rest.rsplit_once('-') {
        Some((head, tail)) if tail == minecraft_version => head.to_string(),
        _ => rest.to_string(),
    };
    if version.is_empty() {
        return Err(CatalogError::UnsupportedLoader { raw: id.into() });
    }
    Ok((loader, version))
}
```

`CatalogError` via `thiserror`. `EngineError::Catalog(#[from] crate::catalog::CatalogError)`.

`ProviderId` as in spec. `ContentClass::dest_folder` as in spec.

Re-export from `lib.rs` the types the UI will need: `CatalogQuery`, `CatalogProject`, `CatalogError`, `ContentClass`, `ProviderId`, `CatalogProjectId`, `CatalogFile`, `CatalogFileFilter`, `CatalogPage`, `CatalogSort`, `CatalogProjectDetail`, `CatalogCategory`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine catalog::`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/engine/Cargo.toml crates/engine/src/catalog crates/engine/src/error.rs crates/engine/src/lib.rs
git commit -m "feat(engine): add catalog types and manifest loader parser"
```

---

### Task 7: `CatalogProvider` + Engine dispatch

**Files:**
- Modify: `crates/engine/src/catalog/provider.rs`
- Modify: `crates/engine/src/catalog/query.rs` (create)
- Modify: `crates/engine/src/lib.rs` (`Engine` fields, `add_provider`, `catalog_*`)

**Interfaces:**
- Consumes: types from Task 6
- Produces: `#[async_trait] pub trait CatalogProvider` with the spec methods; `Engine::add_provider`; `catalog_categories` / `catalog_search` / `catalog_project` / `catalog_files` / `catalog_file`

- [ ] **Step 1: Write a fake-provider dispatch test**

In `catalog/query.rs` `#[cfg(test)]`, copy the sync `test_engine` helper from `content.rs`. Build `Engine` with `MemoryKeychain`, `add_provider(Fake)`, call `catalog_search`.

```rust
struct Fake;

#[async_trait]
impl CatalogProvider for Fake {
    fn id(&self) -> ProviderId { ProviderId::CURSEFORGE }
    fn label(&self) -> &'static str { "Fake" }
    fn supports(&self, class: ContentClass) -> bool { class == ContentClass::Mods }
    fn set_credentials(&self, _: CatalogCredentials) {}
    fn has_credentials(&self) -> bool { true }
    async fn categories(&self, _: ContentClass) -> Result<Vec<CatalogCategory>, CatalogError> { Ok(vec![]) }
    async fn search(&self, q: &CatalogQuery) -> Result<CatalogPage<CatalogProject>, CatalogError> {
        Ok(CatalogPage { items: vec![CatalogProject {
            provider: ProviderId::CURSEFORGE,
            id: CatalogProjectId("1".into()),
            slug: "jei".into(),
            name: q.search.clone().unwrap_or_default(),
            summary: String::new(),
            authors: vec![],
            download_count: 0,
            logo_url: None,
            class: ContentClass::Mods,
        }], index: 0, page_size: 20, total: 1 })
    }
    // remaining methods: unimplemented or return NotFound
}

#[tokio::test]
async fn catalog_search_dispatches() {
    let (_root, engine) = test_engine();
    engine.add_provider(Arc::new(Fake));
    let page = engine.catalog_search(&CatalogQuery {
        class: ContentClass::Mods,
        provider: ProviderId::CURSEFORGE,
        search: Some("jei".into()),
        category_ids: vec![],
        game_version: None,
        loader: None,
        sort: CatalogSort::Popularity,
        index: 0,
        page_size: 20,
    }).await.unwrap();
    assert_eq!(page.items[0].name, "jei");
}

#[tokio::test]
async fn unknown_provider_errors() {
    let (_root, engine) = test_engine();
    let err = engine.catalog_categories(ProviderId("modrinth"), ContentClass::Mods).await.unwrap_err();
    assert!(matches!(err, EngineError::Catalog(CatalogError::UnknownProvider)));
}
```

Stub every other trait method with `Err(CatalogError::NotFound { kind: CatalogResource::Project, id: "-".into() })` so the impl is complete.

- [ ] **Step 2: Run — fail**

Run: `cargo test -p kmine-engine catalog::query -- --nocapture`

Expected: fail (no `add_provider`)

- [ ] **Step 3: Implement**

`Engine` gains `providers: parking_lot::Mutex<Vec<Arc<dyn CatalogProvider>>>` (init empty in `from_keychain`).

```rust
fn provider(&self, id: ProviderId) -> Result<Arc<dyn CatalogProvider>, CatalogError> {
    self.providers.lock().iter().find(|p| p.id() == id).cloned().ok_or(CatalogError::UnknownProvider)
}

pub fn add_provider(&self, p: Arc<dyn CatalogProvider>) {
    self.providers.lock().push(p);
}

pub async fn catalog_search(&self, query: &CatalogQuery) -> Result<CatalogPage<CatalogProject>, EngineError> {
    Ok(self.provider(query.provider)?.search(query).await?)
}
```

Same pattern for the other `catalog_*` methods.

`set_credentials` / `has_credentials` use interior mutability in real adapters; Fake is a unit struct.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine catalog::`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/catalog crates/engine/src/lib.rs
git commit -m "feat(engine): dispatch catalog queries to injected providers"
```

---

### Task 8: Hourly key refresh

**Files:**
- Create: `crates/engine/src/catalog/key.rs`
- Modify: `crates/engine/src/lib.rs` (`CatalogBackend` fields, `start_catalog_key_refresh`, `refresh_catalog_key_once`)

**Interfaces:**
- Consumes: `Store::get_secret` / `put_secret`, `ProviderId::CURSEFORGE`, `CatalogCredentials::ApiKey`
- Produces:
  - secret id `catalog/curseforge`, plaintext `{"apiKey":"...","updatedAt":<ms>}`
  - `Engine::start_catalog_key_refresh(&self)`
  - `Engine::refresh_catalog_key_once(&self) -> Result<(), EngineError>` (also used by the interval; tests call it)
  - env defaults: `KMINE_BACKEND_URL=http://127.0.0.1:8787`, optional `KMINE_BACKEND_TOKEN`

- [ ] **Step 1: Write wiremock tests**

```rust
#[tokio::test]
async fn refresh_writes_secret_and_sets_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/get_cf_api_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "apiKey": "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm",
            "source": "test"
        })))
        .mount(&server).await;
    let (_root, engine) = test_engine();
    let fake = Arc::new(CredFake::default());
    engine.add_provider(fake.clone());
    engine.set_catalog_backend_url(server.uri());
    engine.refresh_catalog_key_once().await.unwrap();
    assert!(fake.has_credentials());
    let raw = engine.store.lock().get_secret(&engine.master_key, "catalog/curseforge").unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(v["apiKey"], "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm");
}

#[tokio::test]
async fn refresh_503_keeps_old_secret() {
    // first 200, then 503; apiKey unchanged
}

#[tokio::test]
async fn refresh_error_display_hides_key() {
    // 200 then parse; EngineError/CatalogError to_string must not contain $2a$10$
}
```

`CredFake`: `Mutex<bool>` for credentials, `id()` = `CURSEFORGE`.

`set_catalog_backend_url` is `pub` (used by tests; harmless in prod). Default URL from env at `Engine::from_keychain`.

- [ ] **Step 2: Run — fail**

Run: `cargo test -p kmine-engine catalog::key -- --nocapture`

Expected: fail

- [ ] **Step 3: Implement**

On `Engine`:

```rust
catalog_backend_url: parking_lot::Mutex<String>,
catalog_backend_token: Option<String>,
```

Init: `std::env::var("KMINE_BACKEND_URL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "http://127.0.0.1:8787".into())`, trim trailing `/`. Token from `KMINE_BACKEND_TOKEN`.

`refresh_catalog_key_once`:

1. `provider(CURSEFORGE)?` — if `UnknownProvider`, return `Ok(())` (start_refresh no-ops the same way).
2. GET `{url}/get_cf_api_key` with `HttpFiles` (or `reqwest` via existing client). Add `Authorization: Bearer` if token set. Timeouts: existing HTTP client is fine.
3. Non-200 → `CatalogError::Http { url, status }` (do **not** put body in the error).
4. Parse `{ apiKey, source? }`. Empty `apiKey` → `CatalogError::Message("empty api key")`.
5. `put_secret(master_key, "catalog/curseforge", json)`. Then `set_credentials(ApiKey(apiKey))`.

`start_catalog_key_refresh`:

1. If no CURSEFORGE provider, return.
2. If secret exists, `set_credentials` from it.
3. If no secret, `refresh_catalog_key_once().await` (ignore err).
4. `self.rt.spawn` loop: `interval(3600s)`, on tick `refresh_catalog_key_once().await` — **drop errors**, no `Event::Error`.

Apply existing secret on start **before** the first GET so a down backend still unlocks the catalog.

Never include `apiKey` in `Debug` of engine fields (do not store the raw key on `Engine`, only in secrets + provider mutex).

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine catalog::`

Expected: PASS. Confirm `to_string()` assertions.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/catalog/key.rs crates/engine/src/lib.rs crates/engine/src/catalog/mod.rs
git commit -m "feat(engine): refresh CurseForge key hourly into secrets"
```

---

### Task 9: Blob cache + `install_pack`

**Files:**
- Create: `crates/engine/src/catalog/cache.rs`
- Create: `crates/engine/src/catalog/install.rs`
- Modify: `crates/engine/src/paths.rs` — `cache_catalog_files`, `cache_catalog_images`; create them in `create_dirs`
- Modify: `crates/engine/src/lib.rs` — `installing: parking_lot::Mutex<bool>`, `install_pack`, `cache_remote_image`

**Interfaces:**
- Consumes: `CatalogProvider::{file, download, parse_pack, walk_overrides}`, `create_instance`, `delete_instance`, `safe_join`
- Produces: `Engine::install_pack(...) -> Result<InstanceId, EngineError>` as in the spec; engine-wide install lock; `cache/catalog/files/{provider}/{file_id}/{file_name}`

- [ ] **Step 1: Write pack install tests with an in-memory `FakePack`**

`FakePack` hardcodes `parse_pack` (does not open a zip):

- `download` of pack file `"pack"` → `CatalogBlob { file_name: "pack.zip", bytes: b"zip", sha1: None }`
- `parse_pack` → `PackManifestSpec { name: "SF".into(), version: "1".into(), minecraft_version: "1.20.1".into(), loader: Loader::Forge, loader_version: Some("47.4.0".into()), files: vec![PackManifestFileSpec { project_id: CatalogProjectId("1".into()), file_id: "2".into(), required: true, class: ContentClass::Mods }] }`
- `download` of `"2"` → `CatalogBlob { file_name: "jei.jar", bytes: b"jar", sha1: None }`. For the cancel test, this call waits on `tokio::sync::Notify` until the test fires it.
- `walk_overrides` → `config/a.toml` / `b"ok"`, or `../escape` in the escape test.

Copy the sync `test_engine` helper from `content.rs` into this module’s `#[cfg(test)]`. Copy `NoopProgress` from `launch/mod.rs` tests if it is private there.

```rust
#[tokio::test]
async fn install_pack_writes_mods_and_overrides() {
    let (_root, engine) = test_engine();
    engine.add_provider(Arc::new(FakePack::ok()));
    let id = engine
        .install_pack(
            ProviderId::CURSEFORGE,
            &CatalogProjectId("p".into()),
            "pack",
            None,
            &NoopProgress,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let row = engine.get_instance(id).unwrap().unwrap();
    assert_eq!(row.loader, Loader::Forge);
    assert_eq!(row.loader_version.as_deref(), Some("47.4.0"));
    let mc = engine.paths.instance_minecraft(&row.slug);
    assert_eq!(std::fs::read(mc.join("mods/jei.jar")).unwrap(), b"jar");
    assert_eq!(std::fs::read(mc.join("config/a.toml")).unwrap(), b"ok");
}

#[tokio::test]
async fn install_pack_cancel_after_create_deletes_instance() {
    let (_root, engine) = test_engine();
    let fake = Arc::new(FakePack::block_on_file_2());
    engine.add_provider(fake.clone());
    let cancel = CancellationToken::new();
    let cancel2 = cancel.clone();
    let engine2 = /* same Engine cannot move; use Arc<Engine> if tests wrap it, else call from spawned task via pointer — construct Engine with open_with_keychain and wrap Arc */;
    // Preferred shape: let engine = Arc::new(engine); add_provider on &*engine;
    let join = tokio::spawn({
        let engine = Arc::clone(&engine);
        let cancel = cancel.clone();
        async move {
            engine
                .install_pack(
                    ProviderId::CURSEFORGE,
                    &CatalogProjectId("p".into()),
                    "pack",
                    None,
                    &NoopProgress,
                    &cancel,
                )
                .await
        }
    });
    while engine.list_instances().unwrap().is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    cancel2.cancel();
    fake.unblock();
    let err = join.await.unwrap().unwrap_err();
    assert!(matches!(err, EngineError::Cancelled));
    assert!(engine.list_instances().unwrap().is_empty());
}

#[tokio::test]
async fn install_pack_rejects_override_escape() {
    let (_root, engine) = test_engine();
    engine.add_provider(Arc::new(FakePack::escape_override()));
    let err = engine
        .install_pack(
            ProviderId::CURSEFORGE,
            &CatalogProjectId("p".into()),
            "pack",
            None,
            &NoopProgress,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(engine.list_instances().unwrap().is_empty());
    let _ = err;
}

#[tokio::test]
async fn install_pack_while_locked_is_busy() {
    let (_root, engine) = test_engine();
    *engine.installing.lock() = true;
    let err = engine
        .install_pack(
            ProviderId::CURSEFORGE,
            &CatalogProjectId("p".into()),
            "pack",
            None,
            &NoopProgress,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::InstanceBusy));
}
```

`Engine` is not `Clone`. For the cancel test put `Engine` in `Arc` (`open_with_keychain` already returns `Engine`; `Arc::new` works if methods take `&self`). `add_provider` is on `&self`.

- [ ] **Step 2: Run — fail**

Run: `cargo test -p kmine-engine catalog::install -- --nocapture`

Expected: fail

- [ ] **Step 3: Implement cache + install_pack**

`cache.rs`:

```rust
pub fn blob_path(paths: &LauncherPaths, provider: ProviderId, file_id: &str, file_name: &str) -> PathBuf {
    paths.cache_catalog_files.join(provider.0).join(file_id).join(file_name)
}

pub fn put_blob(path: &Path, blob: &CatalogBlob) -> Result<(), CatalogError> {
    if path.is_file() {
        let existing = std::fs::read(path).map_err(|e| CatalogError::Message(e.to_string()))?;
        if let Some(exp) = blob.sha1.as_deref() {
            if sha1_hex(&existing) == exp.to_ascii_lowercase() {
                return Ok(());
            }
        } else if !existing.is_empty() {
            return Ok(());
        }
    }
    if let Some(exp) = blob.sha1.as_deref() {
        let actual = sha1_hex(&blob.bytes);
        if actual != exp.to_ascii_lowercase() {
            return Err(CatalogError::Checksum {
                file_id: path.file_name().unwrap().to_string_lossy().into(),
                expected: exp.to_string(),
                actual,
            });
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CatalogError::Message(e.to_string()))?;
    }
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, &blob.bytes).map_err(|e| CatalogError::Message(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| CatalogError::Message(e.to_string()))?;
    Ok(())
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    hex::encode(Sha1::digest(bytes))
}
```

`install_pack`:

1. `try_lock installing` else `InstanceBusy`.
2. `file` + `download` + `put_blob` (progress `"Pack zip"`).
3. `parse_pack`.
4. `create_instance` (name override or manifest name).
5. For each required file: download, `put_blob`, copy into dest from `class.dest_folder()`. Progress `"Files {i}/{n}"`. Missing dest → error.
6. `walk_overrides`: `safe_join(instance_minecraft, relative_path)` — already rejects `..`. `create_dir_all` parent, write bytes. Progress `"Overrides {i}/{total_or_0}"`.
7. `InstancesChanged`, unlock, return id.

On any error after step 4: `delete_instance`, unlock, return the error. Map `Canceled` token to `EngineError::Cancelled`.

`cache_remote_image`: GET url (no API key), `cache/catalog/images/{hex sha1 of url}{ext}`. Ext from `Content-Type` or url path (`.png`/`.jpg`/`.webp`, default `.img`).

Add path fields:

```rust
pub cache_catalog_files: PathBuf,  // cache/catalog/files
pub cache_catalog_images: PathBuf, // cache/catalog/images
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine catalog::`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/catalog crates/engine/src/paths.rs crates/engine/src/lib.rs
git commit -m "feat(engine): install catalog packs with rollback"
```

---

### Task 10: `install_content`

**Files:**
- Modify: `crates/engine/src/catalog/install.rs`
- Modify: `crates/engine/src/lib.rs`

**Interfaces:**
- Consumes: `CatalogProvider::{project, file, download, resolve_required_deps}`
- Produces: `Engine::install_content(...)` as in the spec

- [ ] **Step 1: Tests**

```rust
#[tokio::test]
async fn install_content_mods_on_vanilla_fails() { /* create vanilla; install Mods → Catalog Message */ }

#[tokio::test]
async fn install_content_writes_root_and_required_dep() {
    // Fake file + resolve_required_deps returns one extra CatalogFile class Mods
    // both jars in mods/
}

#[tokio::test]
async fn install_content_overwrites_disabled_and_stays_disabled() {
    // write foo.jar.disabled first; install foo.jar; only foo.jar.disabled remains, enabled=false
}

#[tokio::test]
async fn install_content_while_preparing_is_busy() {
    // insert instance id into engine.preparing; install_content → InstanceBusy
}
```

- [ ] **Step 2: Run — fail**

Run: `cargo test -p kmine-engine catalog::install -- --nocapture`

Expected: fail on missing `install_content`

- [ ] **Step 3: Implement**

1. Same install lock + if `preparing.contains(id)` → `InstanceBusy`.
2. `catalog_project` (or use `file.class`) — if `class == Mods && row.loader == Vanilla` → `CatalogError::Message("vanilla instance cannot install mods")`.
3. `file` + `download` + `put_blob`.
4. `resolve_required_deps(&[root], &row.minecraft_version, loader_opt)` where `loader_opt` is `None` for Vanilla else `Some(row.loader)`.
5. Write root then deps. Dest from `file.class.dest_folder()`. If `dest.join(name + ".disabled")` exists, write there; else write `name`. Use `.part` + rename. Delete `.part` on error.
6. Do **not** delete successful files on later failure. Unlock.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine catalog::`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/catalog/install.rs crates/engine/src/lib.rs
git commit -m "feat(engine): install catalog content into existing instances"
```

---

### Task 11: `CurseForgeProvider` + wire-up

**Files:**
- Create: `src/providers/mod.rs`
- Create: `src/providers/curseforge.rs`
- Modify: `src/main.rs` (or `src/app.rs` `KmineApp::new`) — `add_provider` + `start_catalog_key_refresh`
- Modify: `Cargo.toml` (root) — `kmine-curseforge = { path = "crates/curseforge" }`, `async-trait` if the impl is in kmine
- Create: `src/providers/curseforge.rs` unit tests using `crates/curseforge/tests/fixtures/mod_jei.json`

**Interfaces:**
- Consumes: `kmine_curseforge::{Client, SearchQuery, ClassId, ...}`, `parse_manifest_loader`
- Produces: `CurseForgeProvider` implementing `CatalogProvider`

- [ ] **Step 1: Mapping tests (no HTTP)**

```rust
#[test]
fn jei_fixture_maps_to_mods() {
    let raw = include_str!("../../../crates/curseforge/tests/fixtures/mod_jei.json");
    let envelope: serde_json::Value = serde_json::from_str(raw).unwrap();
    let m: kmine_curseforge::Mod = serde_json::from_value(envelope["data"].clone()).unwrap();
    let p = map_mod(&m);
    assert_eq!(p.class, ContentClass::Mods);
    assert_eq!(p.id.0, m.id.to_string());
}

#[test]
fn class_support() {
    assert!(class_id_to_content(ClassId::MODPACKS) == Some(ContentClass::Modpacks));
    assert!(class_id_to_content(ClassId::WORLDS).is_none());
}
```

If the JEI fixture envelope shape differs, deserialize the way `kmine-curseforge` tests do (read that fixture first).

- [ ] **Step 2: Run — fail**

Run: `cargo test -p kmine providers:: -- --nocapture`

Expected: fail

- [ ] **Step 3: Implement adapter**

```rust
pub struct CurseForgeProvider {
    client: Mutex<Option<Client>>,
}

impl CurseForgeProvider {
    pub fn new() -> Self { Self { client: Mutex::new(None) } }
    fn client(&self) -> Result<Client, CatalogError> {
        self.client.lock().clone().ok_or(CatalogError::Unavailable)
    }
}
```

`set_credentials(ApiKey(k))` → `*self.client.lock() = Some(Client::new(k).map_err(...)?)`.

`supports`: Mods / ResourcePacks / Shaders / Modpacks only.

Map:

| class | `ClassId` |
|---|---|
| Mods | `MODS` |
| ResourcePacks | `RESOURCE_PACKS` |
| Shaders | `SHADERS` |
| Modpacks | `MODPACKS` |

`search`: `SearchQuery::new(class).search(...).categories(parsed u32s).game_version(...).loader(map_loader).sort(...).index().page_size()`.

`ModLoaderType` ↔ `Loader` only for Forge/Fabric/NeoForge/Quilt.

`parse_pack`: `PackZip::parse` → `manifest()` → primary loader via `primary_loader()` + `parse_manifest_loader` → `resolve_pack` → `get_mods` for class. Optional rows dropped. Required missing class → `Manifest`.

`walk_overrides`: loop `next_override`.

`resolve_required_deps`: wrap client, map files, fill `class` via `get_mod`/`get_mods`.

`Debug` for `CurseForgeProvider`: `finish_non_exhaustive`, no key.

In `KmineApp::new` (has `Arc<Engine>`):

```rust
engine.add_provider(Arc::new(CurseForgeProvider::new()));
engine.start_catalog_key_refresh();
```

`start_catalog_key_refresh` is sync and spawns; call it on the tokio runtime already running in `main`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine providers::`
Run: `cargo test -p kmine-engine`
Run: `cargo check -p kmine`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/providers src/app.rs src/main.rs Cargo.toml Cargo.lock
git commit -m "feat(ui): inject CurseForge catalog provider"
```

---

### Task 12: Catalog modal + Content Add

**Files:**
- Create: `src/modals/catalog.rs`
- Modify: `src/modals/mod.rs`
- Modify: `src/modals/create_instance.rs` — sixth cell **Modpack**
- Modify: `src/screens/instance_content.rs` — Add on `section_header`
- Modify: `src/chrome.rs` — `section_header` optional action; `sheet_wide` is `sheet(cx).w(px(880.)).max_h(px(720.))`
- Modify: `src/app.rs` — `CatalogModal` state, open/search/install, progress reuse

**Interfaces:**
- Consumes: `Engine::catalog_*`, `install_pack`, `install_content`, `cache_remote_image`
- Produces: create → Modpack opens catalog `ContentClass::Modpacks`; Content Add opens catalog for that folder; Install uses existing `ProgressModal`

- [ ] **Step 1: No GPUI tests. Add `sheet_wide` and `CatalogModal` structs so `cargo check` fails until wired**

```rust
pub enum CatalogTarget {
    NewInstance,
    Instance(InstanceId),
}

pub struct CatalogModal {
    pub provider: ProviderId,
    pub class: ContentClass,
    pub target: CatalogTarget,
    pub search: Entity<InputState>,
    pub categories: Vec<CatalogCategory>,
    pub selected_categories: Vec<String>,
    pub sort: CatalogSort,
    pub page: Option<CatalogPage<CatalogProject>>,
    pub project: Option<CatalogProjectDetail>,
    pub files: Option<CatalogPage<CatalogFile>>,
    pub selected_file: Option<String>,
    pub error: Option<String>,
    pub loading: bool,
    pub no_key: bool,
}
```

- [ ] **Step 2: `cargo check -p kmine` — fail on unused / missing render**

- [ ] **Step 3: Implement UI behavior (spec § UI)**

Create grid: add Modpack cell with `IconName::Folder`. Click: `show_create = false`; `catalog = Some(...)` with `class: Modpacks`, `target: NewInstance`. Dismiss catalog: drop it (do not return to kind grid).

Content: `folder_section` header **Add** unless `loader == Vanilla && folder == Mods`. Click: catalog with that `ContentClass`, `target: Instance(id)`, query `game_version` + `loader` frozen.

Open catalog: spawn `catalog_categories`. On `CatalogError::Unavailable` set `no_key = true` (panel copy: backend `GET /get_cf_api_key` has never returned a key; search disabled). Else load first `catalog_search`.

Search input: on each change increment `search_gen: u64` on `KmineApp`, `cx.spawn` a 300ms sleep, then if `search_gen` is still the same run `catalog_search`. Drop stale replies when `gen != search_gen`.

Category chips: toggle ids, max 10, AND. Sort segmented: Popularity / Updated / Downloads / Name.

List: cards (logo via `cache_remote_image` then `img(path)`), **More** appends next page (`index + page_size` while `< total` and `index + 2*page_size <= 10000`).

Project: back, name, summary, screenshots, website as `open::that(url)` (engine already uses `open`). File rows; one file on first page → preselect. **Install** disabled without selection.

Install:

1. Close catalog (and create).
2. If another install/`progress` exists, ignore (`InstanceBusy`).
3. `CancellationToken` + `ProgressModal` (name = project name).
4. Pack: `install_pack(provider, id, file_id, None, sink, cancel)` → `selected = id`, refresh instances.
5. Add: `install_content(...)` → `reload_content`.
6. Error: `status = err.to_string()` (must not contain the key). Pack rollback is engine’s job.

While `progress` is Some, `open_create` / Add no-op.

`chrome.rs`:

```rust
pub fn sheet_wide(cx: &App) -> impl ParentElement + Styled + IntoElement {
    sheet(cx).w(px(880.)).max_h(px(720.))
}
```

Do not render `description_html`.

- [ ] **Step 4: Check**

Run: `cargo check -p kmine`
Run: `cargo test -p kmine-engine`
Run: `cargo test -p kmine-backend-api`
Run: `cargo test -p kmine`

Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add src/modals/catalog.rs src/modals/mod.rs src/modals/create_instance.rs \
  src/screens/instance_content.rs src/chrome.rs src/app.rs
git commit -m "feat(ui): catalog modal for packs and content add"
```

---

## Self-review (plan vs spec)

| Spec section | Task |
|---|---|
| Loader NeoForge/Quilt + no schema bump | 1 |
| Quilt prepare / merge | 2 |
| NeoForge pick, 47.x legacy URL, processors | 3 |
| Create kind grid (five loaders) | 4 |
| backend-api GET /get_cf_api_key, extract, token, 503 | 5 |
| Types, `CatalogFile.class`, `parse_manifest_loader` | 6 |
| Trait + single-provider dispatch | 7 |
| Hourly key, secrets, no Event on tick, no key in Display | 8 |
| install_pack, rollback, overrides, zip-slip, cache | 9 |
| install_content, vanilla mods, disabled overwrite, deps | 10 |
| Adapter mapping, `Client` after credentials, main inject | 11 |
| Catalog UI, Add, progress, no-key panel, Modpack cell | 12 |
| No HTML description | 12 |
| No provenance / optional deps / Modrinth | (out, no task) |
| `cache_remote_image` | 9 + 12 |

No TBD/TODO left in steps. Signatures use `CatalogError` / `install_pack` consistently. `Engine::Cancelled` is used for user cancel, not a catalog variant.
