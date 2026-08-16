# Warm Prepare and Verify Files Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Warm Play skips full-cache SHA-1, stale-manifest refetch, native re-extract, and Forge processor re-runs; Verify files runs the same `prepare` path without `spawn`.

**Architecture:** `PrepareMode::{Warm, Verify}` is a parameter on `Engine::prepare` and on `HttpFiles` downloads. One pipeline. Stamps skip natives and processors. Unhashed meta uses a 1-hour mtime TTL. The Play tab adds a secondary Verify button that calls `prepare(Verify)` and never `spawn`.

**Tech Stack:** Existing `kmine-engine` + GPUI binary. Tests: `tempfile` + `wiremock`. No live Mojang/meta in default `cargo test`.

**Spec:** `docs/superpowers/specs/2026-08-16-warm-prepare-design.md`

## Global Constraints

- Rust edition **2024**.
- One `prepare` path. Mode is a parameter. No second download stack.
- Play = `prepare(Warm)` then `spawn`. Verify = `prepare(Verify)` only. No `spawn`.
- Verify resolves account and tokens like Play. No account → `AuthRequired`.
- Warm cache: size known and matches → no SHA-1. Size unknown and SHA-1 known → SHA-1 on disk. Neither → exist and non-empty.
- Verify cache: SHA-1 when known (today's behavior).
- Unhashed manifests: do not delete. Warm reuses if mtime < 3600s. Older → re-download. Offline Warm may use a parseable stale file. Verify always downloads; failure is an error even if stale exists.
- Natives stamp: `{natives_dir}/.kmine-natives-ok` body = natives hex without `-sandbox`.
- Processor stamp: `cache/meta/forge-processors/<installer-stem>.ok` body = lowercase installer SHA-1. Do not infer outputs from argv.
- Verify and Play share `preparing`. Overlap → `InstanceBusy`.
- Interrupted Verify: stamps deleted at the start of those steps, rewritten only on success.
- UI copy: `Verify files` / `Verifying files` / `Files verified`. English. No persistent checkbox.
- No default JVM flags. No ETag. No GPUI tests.
- After every task: `cargo test -p kmine-engine` (and `cargo check -p kmine` after UI / `prepare` signature changes).

## File structure

| Path | Responsibility |
|---|---|
| `crates/engine/src/types.rs` | `PrepareMode` |
| `crates/engine/src/lib.rs` | Re-export `PrepareMode` |
| `crates/engine/src/http.rs` | Mode-aware `cache_hit`, `download_*`, `load_meta_json` |
| `crates/engine/src/launch/mod.rs` | `prepare(..., mode)`; stop deleting manifests; pass mode; native/processor stamps |
| `crates/engine/src/java/mod.rs` | Stop deleting `java-all.json`; TTL via `load_meta_json` |
| `crates/engine/src/mojang/assets.rs` | Pass mode into downloads |
| `crates/engine/src/mojang/libraries.rs` | Pass mode; natives stamp helpers; `ensure_natives` |
| `crates/engine/src/fabric/mod.rs` | (call sites stay in launch; no API change required) |
| `crates/engine/src/quilt/mod.rs` | Same |
| `crates/engine/src/forge/mod.rs` | Pass mode into installer downloads |
| `crates/engine/src/forge/processors.rs` | Processor stamp skip |
| `crates/engine/src/neoforge/mod.rs` | Pass mode; TTL on maven meta |
| `crates/engine/src/skin.rs` | Pass `PrepareMode::Warm` (no behavior change: no SHA-1) |
| `src/app.rs` | Play passes `Warm`+spawn; Verify handler |
| `src/game_output.rs` | Play passes `Warm` |
| `src/screens/instance_play.rs` | Secondary **Verify files** under Play |
| `src/modals/progress.rs` | Modal title comes from caller (already does) |

---

### Task 1: `PrepareMode` on `Engine::prepare`

**Files:**
- Modify: `crates/engine/src/types.rs`
- Modify: `crates/engine/src/lib.rs`
- Modify: `crates/engine/src/launch/mod.rs` (`prepare` / `prepare_vanilla` signature + tests)
- Modify: `src/app.rs` (`play_or_stop`)
- Modify: `src/game_output.rs` (`start_instance`)

**Interfaces:**
- Consumes: existing `Engine::prepare(id, progress, cancel, quick_play)`
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareMode {
    Warm,
    Verify,
}

impl Engine {
    pub async fn prepare(
        &self,
        id: InstanceId,
        progress: &dyn ProgressSink,
        cancel: CancellationToken,
        quick_play: Option<QuickPlay>,
        mode: PrepareMode,
    ) -> Result<LaunchPlan, EngineError>;
}
```

`prepare_vanilla` takes the same `mode` and currently ignores it (threaded in later tasks). `Verify` callers pass `quick_play: None`. Re-export `PrepareMode` from `kmine_engine`.

- [ ] **Step 1: Add a unit test that `PrepareMode` is the prepare argument**

In `crates/engine/src/launch/mod.rs` tests, change `prepare_vanilla_offline_errors_without_account` to pass `PrepareMode::Warm` and add:

```rust
#[tokio::test]
async fn prepare_verify_offline_errors_without_account() {
    let engine = test_engine().await;
    let id = engine
        .create_instance(CreateInstance {
            name: "V".into(),
            minecraft_version: "1.21.1".into(),
            loader: Loader::Vanilla,
            loader_version: None,
            icon_png: None,
        })
        .await
        .unwrap();
    let err = engine
        .prepare(
            id,
            &NoopProgress,
            CancellationToken::new(),
            None,
            PrepareMode::Verify,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::NoAccount));
}
```

- [ ] **Step 2: Run the new test — it must fail to compile**

Run: `cargo test -p kmine-engine --lib prepare_verify_offline_errors_without_account --offline`
Expected: compile error: `prepare` takes 4 args, `PrepareMode` not found.

- [ ] **Step 3: Add the enum and thread the argument**

Add `PrepareMode` to `crates/engine/src/types.rs` (next to `LaunchPlan`). Re-export it in `crates/engine/src/lib.rs` `pub use types::{..., PrepareMode, ...}`.

Change `Engine::prepare` and `prepare_vanilla` to take `mode: PrepareMode`. Do not branch on `mode` yet.

Update every `prepare(` call:

- `launch/mod.rs` tests: `PrepareMode::Warm` except the new Verify test
- `src/app.rs` `play_or_stop`: `PrepareMode::Warm`
- `src/game_output.rs` `start_instance`: `PrepareMode::Warm`

`use kmine_engine::PrepareMode` in the binary.

- [ ] **Step 4: Run engine tests**

Run: `cargo test -p kmine-engine --offline`
Expected: PASS.

Run: `cargo check -p kmine --offline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/types.rs crates/engine/src/lib.rs crates/engine/src/launch/mod.rs src/app.rs src/game_output.rs
git commit -m "feat(engine): pass PrepareMode into prepare"
```

---

### Task 2: Mode-aware `cache_hit` and downloads

**Files:**
- Modify: `crates/engine/src/http.rs`
- Modify every `download_sha1` / `download_many` call site listed below (pass `mode`)

**Interfaces:**
- Consumes: `PrepareMode` from Task 1
- Produces:

```rust
impl HttpFiles {
    pub async fn download_sha1(
        &self,
        url: &str,
        dest: &Path,
        expected_sha1: Option<&str>,
        cancel: &CancellationToken,
        mode: PrepareMode,
    ) -> Result<(), EngineError>;

    pub async fn download_many(
        &self,
        jobs: Vec<DownloadJob>,
        title: &str,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
        mode: PrepareMode,
    ) -> Result<(), EngineError>;
}

pub(crate) fn cache_hit(
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
    mode: PrepareMode,
) -> Result<bool, EngineError>;
```

Rules (exact):

1. Missing, empty, or size mismatch (when size known) → `false`.
2. Warm + size known and matches → `true` (do not open the file for SHA-1).
3. Warm + size unknown + SHA-1 known → `hash_file == expected`.
4. Warm + neither size nor SHA-1 → `true` if exist and non-empty.
5. Verify + SHA-1 known → `hash_file == expected`.
6. Verify + SHA-1 unknown → same as (4).

`download_job` takes `mode` and passes it to `cache_hit`.

Call sites after this task (all `Warm` except later launch/java/assets/libraries get the `prepare` mode in Task 3–4):

- `http.rs` tests → `PrepareMode::Verify` for existing SHA-1 tests (preserves “always hash when sha1 set”)
- `skin.rs` → `Warm`
- `java/mod.rs` → `Warm` for now (Task 3 switches Java to the prepare mode)
- `neoforge/mod.rs` → `Warm` for now
- `mojang/assets.rs` → add `mode` param to `fetch_assets`, pass through
- `mojang/libraries.rs` → add `mode` to `fetch_libraries` / `fetch_client`
- `launch/mod.rs` → pass the `prepare` mode
- `forge/mod.rs` → add `mode` to `prepare_forge` / `fetch_installer_*`

- [ ] **Step 1: Write failing unit tests for `cache_hit`**

In `crates/engine/src/http.rs` tests, add (need `use crate::types::PrepareMode`):

```rust
#[test]
fn warm_size_match_does_not_need_sha1() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.bin");
    std::fs::write(&dest, b"wrong-bytes-same-len!").unwrap();
    // 21 bytes. SHA-1 of "abc" would miss. Warm + matching size must still hit.
    assert!(cache_hit(&dest, Some("deadbeef"), Some(21), PrepareMode::Warm).unwrap());
}

#[test]
fn verify_size_match_still_checks_sha1() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.bin");
    std::fs::write(&dest, b"wrong-bytes-same-len!").unwrap();
    assert!(!cache_hit(&dest, Some("deadbeef"), Some(21), PrepareMode::Verify).unwrap());
}

#[test]
fn warm_unknown_size_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.bin");
    let body = b"abc";
    std::fs::write(&dest, body).unwrap();
    let hash = sha1_hex(body);
    assert!(cache_hit(&dest, Some(&hash), None, PrepareMode::Warm).unwrap());
    assert!(!cache_hit(&dest, Some("deadbeef"), None, PrepareMode::Warm).unwrap());
}

#[tokio::test]
async fn warm_size_hit_makes_zero_http() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("f.bin");
    std::fs::write(&dest, b"abc").unwrap();
    HttpFiles::new()
        .unwrap()
        .download_sha1(
            &format!("{}/f", server.uri()),
            &dest,
            Some("ffffffff"),
            &CancellationToken::new(),
            PrepareMode::Warm,
        )
        .await
        .unwrap();
}
```

The last test needs `download_sha1` to accept `size`. `download_sha1` does not take size today. For this test use `download_job` via `download_many` with `DownloadJob { size: Some(3), sha1: Some("ffffffff".into()), ... }` and `PrepareMode::Warm`.

Replace the last test with:

```rust
#[tokio::test]
async fn warm_size_hit_makes_zero_http() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("f.bin");
    std::fs::write(&dest, b"abc").unwrap();
    let jobs = vec![DownloadJob {
        url: format!("{}/f", server.uri()),
        dest,
        sha1: Some("ffffffff".into()),
        size: Some(3),
    }];
    HttpFiles::new()
        .unwrap()
        .download_many(
            jobs,
            "Files",
            &NoopProgress,
            &CancellationToken::new(),
            PrepareMode::Warm,
        )
        .await
        .unwrap();
}
```

- [ ] **Step 2: Run the new tests — they must fail**

Run: `cargo test -p kmine-engine --lib warm_size_match_does_not_need_sha1 --offline`
Expected: compile error (`cache_hit` has no `mode`) or the Warm size-match test returns `false` because it still SHA-1s `deadbeef`.

- [ ] **Step 3: Implement `cache_hit` + thread `mode`**

```rust
fn cache_hit(
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
    mode: PrepareMode,
) -> Result<bool, EngineError> {
    let meta = match std::fs::metadata(dest) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(EngineError::io(dest, err)),
    };
    if !meta.is_file() || meta.len() == 0 {
        return Ok(false);
    }
    if expected_size.is_some_and(|size| meta.len() != size) {
        return Ok(false);
    }
    let must_hash = match (mode, expected_sha1, expected_size) {
        (PrepareMode::Warm, _, Some(_)) => false,
        (PrepareMode::Warm, Some(_), None) => true,
        (PrepareMode::Warm, None, None) => false,
        (PrepareMode::Verify, Some(_), _) => true,
        (PrepareMode::Verify, None, _) => false,
    };
    if !must_hash {
        return Ok(true);
    }
    let expected = expected_sha1.expect("must_hash implies sha1");
    Ok(hash_file(dest)? == expected.to_ascii_lowercase())
}
```

Add `mode` to `download_sha1`, `download_many`, `download_job`. Update every call site in the grep list to pass `PrepareMode::Warm`, except existing `http.rs` SHA-1 tests which pass `Verify` so they still hash.

`fetch_assets`, `fetch_libraries`, `fetch_client`, `prepare_forge`, `prepare_neoforge`, `resolve_java_from` gain a `mode: PrepareMode` argument and pass it through. `launch/mod.rs` passes the `prepare` mode into those functions.

Existing `cache_hit_skips_network` (no size, matching sha1): keep as `Verify` or `Warm` — both hash when size is `None` and sha1 is `Some`. Either works.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine --offline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/http.rs crates/engine/src/java/mod.rs crates/engine/src/launch/mod.rs crates/engine/src/mojang/assets.rs crates/engine/src/mojang/libraries.rs crates/engine/src/forge/mod.rs crates/engine/src/neoforge/mod.rs crates/engine/src/skin.rs
git commit -m "feat(engine): skip sha1 on warm size-matching cache hits"
```

---

### Task 3: Unhashed meta TTL (`version_manifest`, `java-all`)

**Files:**
- Modify: `crates/engine/src/http.rs`
- Modify: `crates/engine/src/launch/mod.rs` (version manifest download)
- Modify: `crates/engine/src/java/mod.rs` (`java-all.json`)
- Modify: `crates/engine/src/forge/mod.rs` / `neoforge/mod.rs` (maven-metadata.xml is unhashed — same helper)

**Interfaces:**
- Consumes: `PrepareMode`, `HttpFiles::download_sha1`
- Produces:

```rust
pub const META_TTL: Duration = Duration::from_secs(3600);

impl HttpFiles {
    pub async fn load_meta_json<T: DeserializeOwned>(
        &self,
        url: &str,
        dest: &Path,
        mode: PrepareMode,
        cancel: &CancellationToken,
    ) -> Result<T, EngineError>;
}

pub fn meta_is_fresh(path: &Path, ttl: Duration) -> bool;
```

`meta_is_fresh`: file exists, is a file, `mtime.elapsed() < ttl`. Clock-skew / future mtime → treat as stale (`false`).

`load_meta_json`:

- Verify: `download_sha1(url, dest, None, cancel, Verify)` then parse. Download error → return that error (do not fall back).
- Warm + fresh: parse dest, no HTTP.
- Warm + stale or missing: download. On download error, if dest exists and parses → return that. Else return the download error.

Do **not** `remove_file` before download.

Maven metadata (`maven-metadata.xml`) is XML, not JSON. Add a sibling:

```rust
pub async fn load_meta_bytes(
    &self,
    url: &str,
    dest: &Path,
    mode: PrepareMode,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, EngineError>;
```

Same freshness rules. `load_meta_json` = `load_meta_bytes` + `serde_json::from_slice`.

- [ ] **Step 1: Write failing tests**

In `crates/engine/src/http.rs` tests:

```rust
#[test]
fn meta_is_fresh_respects_ttl() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.json");
    std::fs::write(&path, b"{}").unwrap();
    assert!(meta_is_fresh(&path, Duration::from_secs(3600)));
    let old = std::time::SystemTime::now() - Duration::from_secs(3601);
    filetime_or_set_modified(&path, old);
    assert!(!meta_is_fresh(&path, Duration::from_secs(3600)));
}
```

Do **not** add a `filetime` crate. Use `std::fs::File::set_modified`:

```rust
let file = std::fs::File::options().write(true).open(&path).unwrap();
file.set_modified(std::time::SystemTime::now() - Duration::from_secs(3601))
    .unwrap();
```

```rust
#[tokio::test]
async fn load_meta_json_warm_fresh_zero_http() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("m.json");
    std::fs::write(&dest, br#"{"ok":true}"#).unwrap();
    let v: serde_json::Value = HttpFiles::new()
        .unwrap()
        .load_meta_json(
            &format!("{}/m", server.uri()),
            &dest,
            PrepareMode::Warm,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(v["ok"], true);
}

#[tokio::test]
async fn load_meta_json_verify_always_fetches() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_raw(br#"{"ok":false}"#, "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("m.json");
    std::fs::write(&dest, br#"{"ok":true}"#).unwrap();
    let v: serde_json::Value = HttpFiles::new()
        .unwrap()
        .load_meta_json(
            &format!("{}/m", server.uri()),
            &dest,
            PrepareMode::Verify,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(v["ok"], false);
}

#[tokio::test]
async fn load_meta_json_warm_stale_falls_back_when_download_fails() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("m.json");
    std::fs::write(&dest, br#"{"ok":true}"#).unwrap();
    let file = std::fs::File::options().write(true).open(&dest).unwrap();
    file.set_modified(std::time::SystemTime::now() - Duration::from_secs(4000))
        .unwrap();
    drop(file);
    let v: serde_json::Value = HttpFiles::new()
        .unwrap()
        .load_meta_json(
            &format!("{}/m", server.uri()),
            &dest,
            PrepareMode::Warm,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(v["ok"], true);
}
```

- [ ] **Step 2: Run tests — they must fail**

Run: `cargo test -p kmine-engine --lib load_meta_json_warm_fresh_zero_http --offline`
Expected: `load_meta_json` not found.

- [ ] **Step 3: Implement helper and switch callers**

Implement `meta_is_fresh` + `load_meta_bytes` + `load_meta_json` in `http.rs`.

In `launch/mod.rs` replace:

```rust
if manifest_path.exists() {
    let _ = std::fs::remove_file(&manifest_path);
}
http.download_sha1(VERSION_MANIFEST_URL, &manifest_path, None, cancel).await?;
let manifest: VersionManifest = read_json(&manifest_path)?;
```

with:

```rust
let manifest: VersionManifest = http
    .load_meta_json(VERSION_MANIFEST_URL, &manifest_path, mode, cancel)
    .await?;
```

In `java/mod.rs` delete the `if all_path.exists() { remove_file }` block. Load via `load_meta_json` into `JavaAll` (or `load_meta_bytes` + existing parse). `resolve_java_from` already has `mode` from Task 2.

Forge/NeoForge maven metadata: same `load_meta_bytes` instead of delete+`download_sha1`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine --offline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/http.rs crates/engine/src/launch/mod.rs crates/engine/src/java/mod.rs crates/engine/src/forge/mod.rs crates/engine/src/neoforge/mod.rs
git commit -m "feat(engine): cache unhashed meta with a one-hour ttl"
```

---

### Task 4: Fabric and Quilt loader indexes

**Files:**
- Modify: `crates/engine/src/launch/mod.rs` (`merge_fabric_profile`, `merge_quilt_profile`)

**Interfaces:**
- Consumes: `HttpFiles::load_meta_json`, `PrepareMode`
- Produces: indexes cached at

```
cache/meta/fabric-loader-index.json
cache/meta/quilt-loader-index.json
```

Profile JSON (`.../profile/json`) stays `get_json` (version-specific, no TTL file in spec). Only the **loader index** is cached.

Pass `mode` into `merge_fabric_profile` / `merge_quilt_profile`.

- [ ] **Step 1: Write a failing test that Warm does not hit the index URL when the file is fresh**

Add a focused test in `launch/mod.rs` **or** test `load_meta_json` is used by extracting a helper. Prefer a unit-level test of the dest path constant and a wiremock test of `merge_fabric_profile` if you can construct `HttpFiles` + temp `LauncherPaths`.

Simplest path that still locks the contract: export dest helpers next to the merge functions in `launch/mod.rs`:

```rust
fn fabric_index_path(paths: &LauncherPaths) -> PathBuf {
    paths.cache_meta.join("fabric-loader-index.json")
}
fn quilt_index_path(paths: &LauncherPaths) -> PathBuf {
    paths.cache_meta.join("quilt-loader-index.json")
}
```

Then a `#[tokio::test]` that writes a valid Fabric index fixture to that path, points `LOADER_INDEX_URL` — **cannot override the const**. So do not try to call `merge_fabric_profile` against live meta.

Test `load_meta_json` already covers TTL. This task's test is: `merge_fabric_profile` calls `load_meta_json` with `fabric-loader-index.json`.

Add in `launch/mod.rs` tests:

```rust
#[test]
fn fabric_index_cache_path() {
    let paths = LauncherPaths::new(PathBuf::from("/data/kmine"));
    assert!(fabric_index_path(&paths)
        .ends_with("cache/meta/fabric-loader-index.json")
        || fabric_index_path(&paths).ends_with("cache\\meta\\fabric-loader-index.json"));
}

#[test]
fn quilt_index_cache_path() {
    let paths = LauncherPaths::new(PathBuf::from("/data/kmine"));
    assert!(quilt_index_path(&paths)
        .ends_with("cache/meta/quilt-loader-index.json")
        || quilt_index_path(&paths).ends_with("cache\\meta\\quilt-loader-index.json"));
}
```

- [ ] **Step 2: Run tests — they must fail**

Run: `cargo test -p kmine-engine --lib fabric_index_cache_path --offline`
Expected: `fabric_index_path` not found.

- [ ] **Step 3: Implement**

`merge_fabric_profile` needs `paths: &LauncherPaths` **or** a dest `Path`. Today it only has `http`, `row`, `vanilla`, `progress`, `cancel`. Add `paths: &LauncherPaths` and `mode: PrepareMode`.

```rust
let index: FabricLoaderIndex = http
    .load_meta_json(LOADER_INDEX_URL, &fabric_index_path(paths), mode, cancel)
    .await?;
```

Same for Quilt. Update the two call sites in `prepare_vanilla`.

Keep `get_json` for the per-version profile URL.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine --offline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/launch/mod.rs
git commit -m "feat(engine): cache fabric and quilt loader indexes"
```

---

### Task 5: Natives stamp

**Files:**
- Modify: `crates/engine/src/mojang/libraries.rs`
- Modify: `crates/engine/src/launch/mod.rs` (extract loop)

**Interfaces:**
- Consumes: `natives_dir_name`, `PrepareMode`
- Produces:

```rust
pub const NATIVES_STAMP_NAME: &str = ".kmine-natives-ok";

pub fn natives_stamp_hex(artifacts: &[LibraryArtifact]) -> String;
// sha1 of sorted paths — same as natives_dir_name without "-sandbox"

pub fn natives_stamp_valid(natives_dir: &Path, hex: &str) -> bool;
pub fn write_natives_stamp(natives_dir: &Path, hex: &str) -> Result<(), EngineError>;

pub fn ensure_natives(
    artifacts: &[LibraryArtifact],
    natives_dir: &Path,
    mode: PrepareMode,
    progress: &dyn ProgressSink,
    exclude_for: impl FnMut(&str) -> Vec<String>,
) -> Result<(), EngineError>;
```

`natives_stamp_valid`: read `{natives_dir}/.kmine-natives-ok`, trim, compare to `hex`.

Warm + valid stamp → `progress.set("Natives", 1, 1)` (or `0, 0` then done) and return. Do not open jars.

Verify: `std::fs::remove_dir_all(natives_dir)` if it exists, `create_dir_all`, extract every native jar, `write_natives_stamp` only after all extracts succeed.

Warm + missing/invalid stamp: extract as today, then write stamp.

Stamp body is the hex **without** `-sandbox`.

- [ ] **Step 1: Write failing tests**

In `crates/engine/src/mojang/libraries.rs` tests:

```rust
#[test]
fn natives_stamp_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let hex = "abc123";
    assert!(!natives_stamp_valid(dir.path(), hex));
    write_natives_stamp(dir.path(), hex).unwrap();
    assert!(natives_stamp_valid(dir.path(), hex));
    assert!(!natives_stamp_valid(dir.path(), "nope"));
}
```

Warm skip test — no native jar on disk; extract must not run:

```rust
#[test]
fn ensure_natives_warm_skips_when_stamp_matches() {
    let dir = tempfile::tempdir().unwrap();
    let natives = dir.path().join("natives");
    std::fs::create_dir_all(&natives).unwrap();
    let artifacts = vec![LibraryArtifact {
        path: "natives/foo.jar".into(),
        url: String::new(),
        sha1: None,
        size: None,
        extract_natives: true,
    }];
    let hex = natives_stamp_hex(&artifacts);
    write_natives_stamp(&natives, &hex).unwrap();
    ensure_natives(&artifacts, &natives, PrepareMode::Warm, &NoopProgress, |_| {
        panic!("extract should not run");
    })
    .unwrap();
}
```

Add Verify test: stamp present, missing jar → `ensure_natives(..., Verify, ...)` returns `Err` (tried to extract). That proves Verify does not skip.

Need `NoopProgress` in this module's tests (copy the 3-line impl from `http.rs` / `assets.rs`).

- [ ] **Step 2: Run tests — they must fail**

Run: `cargo test -p kmine-engine --lib natives_stamp_round_trip --offline`
Expected: `natives_stamp_valid` not found.

- [ ] **Step 3: Implement and switch the launch loop**

Implement the helpers. Replace the extract `for` loop in `prepare_vanilla` with `ensure_natives(...)` using the existing `exclude_for(&version, path)`.

Verify: delete the natives dir first (including stamp).

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine --offline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/mojang/libraries.rs crates/engine/src/launch/mod.rs
git commit -m "feat(engine): skip native extract when the stamp matches"
```

---

### Task 6: Forge/NeoForge processor stamp

**Files:**
- Modify: `crates/engine/src/forge/processors.rs`
- Modify: `crates/engine/src/launch/mod.rs` (call `run_processors` with mode)
- Modify: `crates/engine/src/paths.rs` only if you add `cache_meta` usage (already exists)

**Interfaces:**
- Consumes: `PrepareMode`, installer path, `LauncherPaths.cache_meta`
- Produces:

```rust
pub fn processor_stamp_path(paths: &LauncherPaths, installer: &Path) -> PathBuf;
// cache/meta/forge-processors/<installer-file-stem>.ok

pub fn installer_sha1(installer: &Path) -> Result<String, EngineError>;
// lowercase hex of the installer jar bytes (reuse http::hash or sha1 in-place)

pub async fn run_processors(
    java: &Path,
    profile: &ForgeInstallProfile,
    paths: &LauncherPaths,
    vanilla_client: &Path,
    cancel: &CancellationToken,
    mode: PrepareMode,
) -> Result<(), EngineError>;
```

Warm: if stamp file exists and its trimmed body == `installer_sha1(installer)` → return `Ok(())` without calling `run_one`.

Verify: `std::fs::remove_file(stamp)` if present (ignore NotFound), run the existing loop, `write_stamp` only if every `run_one` succeeded.

Warm + no stamp: run loop, then write stamp (first Play after upgrade).

Do not parse processor argv for outputs.

- [ ] **Step 1: Write failing tests**

In `crates/engine/src/forge/processors.rs` tests (existing `run_processors_skips_server_only` lives here). Add:

```rust
#[test]
fn processor_stamp_path_uses_installer_stem() {
    let paths = LauncherPaths::new(PathBuf::from("/data/kmine"));
    let stamp = processor_stamp_path(
        &paths,
        Path::new("/cache/forge-1.21.1-52.0.0-installer.jar"),
    );
    assert!(
        stamp.ends_with("cache/meta/forge-processors/forge-1.21.1-52.0.0-installer.ok")
            || stamp.ends_with("cache\\meta\\forge-processors\\forge-1.21.1-52.0.0-installer.ok")
    );
}

#[tokio::test]
async fn run_processors_warm_skips_when_stamp_matches() {
    let root = tempfile::tempdir().unwrap();
    let paths = LauncherPaths::new(root.path().to_path_buf());
    paths.create_dirs().unwrap();
    let installer = root.path().join("inst.jar");
    std::fs::write(&installer, b"installer-bytes").unwrap();
    let sha = installer_sha1(&installer).unwrap();
    let stamp = processor_stamp_path(&paths, &installer);
    std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
    std::fs::write(&stamp, sha).unwrap();

    let profile = ForgeInstallProfile {
        processors: vec![ForgeProcessor {
            sides: vec!["client".into()],
            jar: "net.minecraftforge:installertools:1.0.0".into(),
            classpath: vec![],
            args: vec![],
            ..Default::default()
        }],
        installer_path: installer.clone(),
        ..Default::default()
    };
    // java path can be fake: skip must happen before spawn
    run_processors(
        Path::new("/no/java"),
        &profile,
        &paths,
        Path::new("/no/client.jar"),
        &CancellationToken::new(),
        PrepareMode::Warm,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn run_processors_verify_deletes_stamp_and_runs() {
    let root = tempfile::tempdir().unwrap();
    let paths = LauncherPaths::new(root.path().to_path_buf());
    paths.create_dirs().unwrap();
    let installer = root.path().join("inst.jar");
    std::fs::write(&installer, b"installer-bytes").unwrap();
    let sha = installer_sha1(&installer).unwrap();
    let stamp = processor_stamp_path(&paths, &installer);
    std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
    std::fs::write(&stamp, &sha).unwrap();

    let profile = ForgeInstallProfile {
        processors: vec![ForgeProcessor {
            sides: vec!["client".into()],
            jar: "net.minecraftforge:installertools:1.0.0".into(),
            classpath: vec![],
            args: vec![],
            ..Default::default()
        }],
        installer_path: installer.clone(),
        ..Default::default()
    };
    let err = run_processors(
        Path::new("/no/java"),
        &profile,
        &paths,
        Path::new("/no/client.jar"),
        &CancellationToken::new(),
        PrepareMode::Verify,
    )
    .await
    .unwrap_err();
    let _ = err;
    assert!(!stamp.exists(), "verify must delete stamp before run");
}
```

Existing `run_processors_skips_server_only` must pass `PrepareMode::Warm` (or Verify — server-only still skips). Update that call.

- [ ] **Step 2: Run tests — they must fail**

Run: `cargo test -p kmine-engine --lib processor_stamp_path_uses_installer_stem --offline`
Expected: `processor_stamp_path` not found.

- [ ] **Step 3: Implement stamp skip**

At the top of `run_processors`, after cancel check:

```rust
let stamp = processor_stamp_path(paths, profile.installer_path.as_path());
let current = installer_sha1(profile.installer_path.as_path())?;
if mode == PrepareMode::Warm {
    if let Ok(body) = std::fs::read_to_string(&stamp) {
        if body.trim() == current {
            return Ok(());
        }
    }
} else {
    match std::fs::remove_file(&stamp) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(EngineError::io(&stamp, err)),
    }
}
```

After the processor loop succeeds:

```rust
if let Some(parent) = stamp.parent() {
    std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
}
std::fs::write(&stamp, current.as_bytes()).map_err(|e| EngineError::io(&stamp, e))?;
```

Update `launch/mod.rs` `run_processors(..., mode)`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine --offline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/forge/processors.rs crates/engine/src/launch/mod.rs
git commit -m "feat(engine): skip forge processors when the installer stamp matches"
```

---

### Task 7: Interrupted Verify + overlap tests

**Files:**
- Modify: `crates/engine/src/launch/mod.rs` tests
- Modify: `crates/engine/src/forge/processors.rs` tests if the cancel case is easier there

**Interfaces:**
- Consumes: stamp helpers from Tasks 5–6, `begin_prepare`
- Produces: no new API

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test]
async fn prepare_overlap_is_instance_busy() {
    let engine = test_engine().await;
    // insert a fake preparing id the same way begin_prepare does
    let id = engine
        .create_instance(CreateInstance {
            name: "Busy".into(),
            minecraft_version: "1.21.1".into(),
            loader: Loader::Vanilla,
            loader_version: None,
            icon_png: None,
        })
        .await
        .unwrap();
    engine.preparing.lock().insert(id);
    let err = engine
        .prepare(
            id,
            &NoopProgress,
            CancellationToken::new(),
            None,
            PrepareMode::Verify,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::InstanceBusy));
}
```

`preparing` is `pub(crate)` — tests in `launch/mod.rs` are inside `Engine` impl module? They are `mod tests` under `launch/mod.rs`, so they need `engine.preparing` — `Engine.preparing` is `pub(crate)` on the struct in `lib.rs`. `launch/mod.rs` is `pub mod launch` inside the crate, so `engine.preparing.lock().insert(id)` works.

For interrupted Verify natives: unit-test `ensure_natives` — Verify starts by deleting the dir; cancel is `prepare`-level. Spec: “cancel after stamp delete → next Warm does not treat stamps as valid”.

```rust
#[test]
fn verify_clears_natives_stamp_before_extract() {
    let dir = tempfile::tempdir().unwrap();
    let natives = dir.path().join("natives");
    std::fs::create_dir_all(&natives).unwrap();
    write_natives_stamp(&natives, "abc123").unwrap();
    let artifacts = vec![LibraryArtifact {
        path: "missing/native.jar".into(),
        url: String::new(),
        sha1: None,
        size: None,
        extract_natives: true,
    }];
    let err = ensure_natives(
        &artifacts,
        &natives,
        PrepareMode::Verify,
        &NoopProgress,
        |_| vec![],
    )
    .unwrap_err();
    let _ = err;
    assert!(!natives_stamp_valid(&natives, "abc123"));
}
```

If Task 5 already deletes the dir at the start of Verify, this test should **pass** now. If it passes, keep it as a regression lock. If Verify currently extracts without deleting first, implement the delete.

Same for processors: Task 6 Verify test already asserts stamp is gone after a failed run. Add:

```rust
#[test]
fn warm_does_not_skip_processors_after_verify_deleted_stamp() {
    // stamp absent → Warm must not return Ok before run_one
    // reuse run_processors_verify_deletes_stamp_and_runs leftover:
    // after Verify error, Warm with same profile + fake java also errors (tries to run)
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p kmine-engine --lib prepare_overlap_is_instance_busy --offline`
Expected: FAIL or PASS. If `preparing` insert works and `begin_prepare` checks it, this should pass with current code — still add it, it locks the spec.

- [ ] **Step 3: Fix any gap**

If overlap does not return `InstanceBusy`, `begin_prepare` is the fix (already implemented — do not rewrite). If Verify leaves a stamp after a failed extract, delete stamp/dir **before** extract (Task 5).

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-engine --offline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/launch/mod.rs crates/engine/src/mojang/libraries.rs crates/engine/src/forge/processors.rs
git commit -m "test(engine): lock verify overlap and interrupted stamps"
```

---

### Task 8: Play-tab **Verify files**

**Files:**
- Modify: `src/screens/instance_play.rs` (`launch_hero`)
- Modify: `src/app.rs` (handler; do not `spawn`)

**Interfaces:**
- Consumes: `Engine::prepare(..., PrepareMode::Verify)`, existing `ProgressModal`, `EventProgressSink`, `CancellationToken`
- Produces: no new engine API

UI:

- Under the primary Play / Stop button, a secondary `Button::new("verify-files")` labeled `Verify files`. Not `style_cta`. Not `.primary()`.
- Disabled when `preparing || instance.running`.
- Click → same progress modal machinery as Play, title `Verifying files`.
- Success: `progress = None`, `status = "Files verified"`, **do not** `spawn`, **do not** `open_game_output`, **do not** increment playtime (engine already does not).
- Error: same match as Play (`NoAccount` opens accounts, `Cancelled` clears status, else `err.to_string()`).
- Cancel: existing cancel path.

`launch_hero` gains `on_verify: impl Fn(...)`.

```rust
pub fn launch_hero(
    instance: &InstanceSummary,
    preparing: bool,
    on_play: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_verify: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement
```

Layout: wrap Play + Verify in `v_flex().gap_2()` so Verify sits under Play.

`KmineApp::verify_files(&mut self, id, cx)`:

```rust
fn verify_files(&mut self, id: InstanceId, cx: &mut Context<Self>) {
    let Some(instance) = self.instances.iter().find(|i| i.id == id).cloned() else {
        return;
    };
    if instance.running || self.progress.is_some() {
        return;
    }
    let cancel = CancellationToken::new();
    self.cancel = Some(cancel.clone());
    self.progress = Some(ProgressModal {
        id,
        name: instance.name.clone(),
        title: "Verifying files".into(),
        done: 0,
        total: 0,
    });
    self.status.clear();
    cx.notify();
    let engine = self.engine.clone();
    let rt = self.rt.clone();
    cx.spawn(async move |this: WeakEntity<Self>, cx| {
        let prepared = rt
            .spawn(async move {
                let sink = EventProgressSink::new(engine.event_sender(), id);
                engine
                    .prepare(id, &sink, cancel, None, PrepareMode::Verify)
                    .await
                    .map(|_| ())
            })
            .await;
        match prepared {
            Ok(Ok(())) => {
                this.update(cx, |this, cx| {
                    this.progress = None;
                    this.cancel = None;
                    this.status = "Files verified".into();
                    cx.notify();
                })
                .ok();
            }
            Ok(Err(err)) => {
                this.update(cx, |this, cx| {
                    this.progress = None;
                    this.cancel = None;
                    match err {
                        EngineError::NoAccount => {
                            this.refresh_accounts();
                            this.show_settings = false;
                            this.show_accounts = true;
                            this.status = EngineError::NoAccount.to_string();
                        }
                        EngineError::Cancelled => this.status.clear(),
                        other => this.status = other.to_string(),
                    }
                    cx.notify();
                })
                .ok();
            }
            Err(err) => {
                this.update(cx, |this, cx| {
                    this.progress = None;
                    this.cancel = None;
                    this.status = err.to_string();
                    cx.notify();
                })
                .ok();
            }
        }
    })
    .detach();
}
```

Play path: keep `prepare(..., Warm)` then `spawn`.

No GPUI tests. Lock the contract with `cargo check -p kmine`.

- [ ] **Step 1: Change `launch_hero` signature so `cargo check -p kmine` fails**

Add the `on_verify` parameter and the button. Do not wire `app.rs` yet.

- [ ] **Step 2: Run check — it must fail**

Run: `cargo check -p kmine --offline`
Expected: `launch_hero` argument count mismatch at `app.rs`.

- [ ] **Step 3: Wire `app.rs`**

Pass `on_verify` that calls `verify_files`. Implement `verify_files` as above. Import `PrepareMode`.

- [ ] **Step 4: Check + engine tests**

Run: `cargo check -p kmine --offline`
Expected: PASS.

Run: `cargo test -p kmine-engine --offline`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/screens/instance_play.rs src/app.rs
git commit -m "feat(ui): add verify files without launching"
```

---

## Self-review (spec coverage)

| Spec requirement | Task |
|---|---|
| `PrepareMode` on `prepare` | 1 |
| Play Warm + spawn / Verify no spawn | 1, 8 |
| Account required for Verify | 1 (`prepare_verify_offline_errors_without_account`) |
| Warm size hit skips SHA-1 | 2 |
| Warm unknown size + SHA-1 hashes | 2 |
| Verify always SHA-1 when known | 2 |
| Do not delete unhashed manifests | 3 |
| 1-hour TTL; stale Warm fallback; Verify always fetch | 3 |
| Fabric/Quilt loader index cache | 4 |
| Natives stamp skip / Verify wipe | 5 |
| Processor stamp / no argv inference | 6 |
| Shared `preparing` → `InstanceBusy` | 7 |
| Interrupted Verify invalidates stamps | 5–7 |
| UI copy and secondary button | 8 |
| No JVM flags / no ETag / no GPUI tests | no task (out) |
| First Warm after upgrade writes stamps | 5, 6 (missing stamp → run then write) |
