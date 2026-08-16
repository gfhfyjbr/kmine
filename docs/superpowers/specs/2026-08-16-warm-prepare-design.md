# kmine Warm Prepare and Verify Files Design

Date: 2026-08-16
Status: drafted from conversation; awaiting file review

This spec amends [2026-08-14-kmine-launcher-design.md](2026-08-14-kmine-launcher-design.md): every Play no longer fully SHA-1s the cache, re-downloads unhashed manifests, re-extracts natives, or re-runs Forge/NeoForge processors. A one-shot **Verify files** action runs the same `prepare` pipeline without `spawn`.

It does not amend catalog, CurseForge, or auth specs.

## Goal

Warm Play should skip work that cannot change if the files on disk are already the right size (or the right SHA-1 when size is unknown). Verify files is a user-triggered full check: hashes, fresh manifests, native re-extract, processor re-run. It does not start the game.

## Decisions (locked)

| Decision | Choice |
|---|---|
| Scope | Engine warm hits + Play-tab **Verify files**. No default JVM flags |
| Verify UX | One-shot. Not a persistent checkbox. Not a global setting |
| Verify action | `prepare(Verify)` only. No `spawn` |
| Play action | `prepare(Warm)` then `spawn` |
| Pipeline | One `prepare` path. Mode is a parameter. No second download stack |
| Account | Verify resolves account and tokens like Play. No account → `AuthRequired` |
| Warm cache hit | Size known and matches → no SHA-1. Size unknown and SHA-1 known → SHA-1 on disk, no network if it matches. Neither size nor SHA-1 → exist and non-empty |
| Verify cache hit | Current SHA-1 behavior |
| Unhashed manifests | Do not delete. Warm reuses if younger than 1 hour. Older → re-download. Offline Warm may use a stale file if it parses |
| Natives | Stamp `{natives_dir}/.kmine-natives-ok` whose body is the natives hex (no `-sandbox`). Warm skips extract if it matches. Verify wipes the dir, extracts, writes stamp |
| Processors | Stamp `cache/meta/forge-processors/<installer-stem>.ok` holding the installer SHA-1. Warm skips all client processors if stamp matches current installer. Verify deletes stamp, runs processors, writes stamp on success |
| Processor outputs | Do not infer output paths from argv |
| Busy | Verify and Play share `preparing`. Two at once on one instance → `InstanceBusy` |
| Interrupted Verify | Stamps for natives/processors are removed at the start of those steps and rewritten only on success. Next Warm rebuilds them |
| UI copy | English. Button `Verify files`. Progress title `Verifying files`. Success status `Files verified` |
| GPUI tests | Out |

## Scope

### In

- `PrepareMode::{Warm, Verify}` on `Engine::prepare`
- Warm / Verify rules in `HttpFiles` cache hits
- 1-hour TTL for `version_manifest_v2.json`, `java-all.json`, Fabric loader index, Quilt loader index
- Native extract skip + stamp
- Forge/NeoForge processor skip + stamp
- Play tab secondary **Verify files** button, same progress modal, no spawn
- Engine unit/integration tests listed below

### Out

- Default Aikar / ZGC / other JVM flag presets
- Persistent "always verify" setting
- Verify without an account
- ETag / `If-None-Match` on manifests
- Parallelizing independent `prepare` steps
- On-disk SHA-1 index besides the two stamps
- Prism-style wipe of natives after the process exits
- GPUI tests
- Changing sandbox, auth, or catalog behavior

## Architecture

```
Play          →  prepare(id, Warm,   quick_play?)  →  spawn
Verify files  →  prepare(id, Verify, None)         →  status only
```

`Engine::prepare` grows a `PrepareMode` argument. Call sites in the binary pass `Warm` (Play / Stop-and-relaunch / game output relaunch) or `Verify`. The engine does not spawn inside `prepare`.

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

`Verify` ignores `quick_play` (callers pass `None`). Auth, version JSON, loaders, Java, client, libraries, natives, assets, logging config, and argument build stay in this function. Mode only changes hit / skip / stamp behavior.

### Cache hits

`HttpFiles::download_job` / `download_many` take `PrepareMode` (or a `CachePolicy` derived from it). One implementation:

1. Missing, empty, or size mismatch (when size is known) → download.
2. **Warm** and size is known and matches → hit. Do not read the file for SHA-1.
3. **Warm** and size is unknown and SHA-1 is known → hash the file. Match → hit. Mismatch → download.
4. **Warm** and neither size nor SHA-1 is known → hit if the file exists and is non-empty (today's unhashed path).
5. **Verify** and SHA-1 is known → hash. Match → hit. Mismatch → download.
6. **Verify** and SHA-1 is unknown → same as (4) if the file exists; otherwise download.

Do not delete `version_manifest_v2.json` or `java-all.json` before download.

### Unhashed meta TTL

These files have no SHA-1 from Mojang in our current calls:

- `cache/meta/version_manifest_v2.json`
- `cache/meta/java-all.json`
- Fabric loader index (store under `cache/meta/`, same pattern as other meta)
- Quilt loader index (same)

Warm: if the file exists, parses, and `mtime` is younger than **3600 seconds**, use it. If older, download and replace. If download fails and a parseable file exists (offline / flaky network), Warm uses the stale file. If there is no file, Warm fails as today.

Verify: always download. Failure is an error even if a stale file exists.

Version JSON, client jar, libraries, assets, log4j config, and Java runtime files already have SHA-1 or size from their indexes. They use the cache-hit table, not this TTL.

### Natives

Directory name is already `sha1(sorted artifact paths)` plus optional `-sandbox`. Stamp path: `{natives_dir}/.kmine-natives-ok`. Contents: the same hex name (without `-sandbox`) so a leftover stamp in the wrong dir cannot match.

Warm: if stamp exists and matches → skip `extract_natives`. Progress `Natives` goes to done immediately.

Verify: delete `{natives_dir}` (or its contents), extract every native jar, write stamp only after every extract succeeds.

A successful Warm extract (first run, or after a missing stamp) also writes the stamp.

### Forge / NeoForge processors

Do not guess outputs from processor argv. Stamp:

```
cache/meta/forge-processors/<installer-file-stem>.ok
```

File body: lowercase hex SHA-1 of the installer jar.

Warm: if stamp exists and equals the current installer SHA-1 → skip the entire client processor loop (`run_one` is not called). Server-only processors stay skipped as today.

Verify: delete the stamp first, run every non-server processor, write the stamp only if all succeed.

A successful first Warm run (no stamp yet) writes the stamp after processors finish.

Installer SHA-1 is the hash of the installer jar on disk (already downloaded via `download_sha1`). If the installer is re-downloaded and the hash changes, Warm does not skip.

### Progress

Warm: when a batch is all hits, set the step to done once (`Libraries`, `Assets`, `Java`, `Natives`). Do not tick thousands of file counters.

Verify: keep current per-file / per-byte progress.

Progress title comes from the UI: Play uses the existing prepare copy; Verify uses `Verifying files`.

## UI

Play tab, under the primary Play / Stop button:

- Secondary **Verify files** (not `style_cta` / not the white pill).
- Disabled when `preparing` or `running`.
- Click opens the existing `ProgressModal` with title `Verifying files`.
- On success: close the modal, set the chrome status to `Files verified`. Do not spawn. Do not increment playtime.
- On error: same error surface as a failed Play (`EngineError` in status / alert).
- Cancel uses the same `CancellationToken` path as Play.

No settings checkbox. No "verify next launch" flag. No new window.

## Error handling

- Same `EngineError` variants. No new error type unless a stamp write fails — then `EngineError::io` on the stamp path.
- Verify hash mismatch → re-download that file. Download failure → error. Do not write natives/processor stamps on failure.
- Cancel mid-Verify after stamps were deleted → next Warm re-extracts natives and/or re-runs processors. That is intended.
- Offline Verify that must fetch an unhashed manifest → error.
- Offline Warm with a parseable stale manifest → success.

## Testing

Engine tests only (wiremock + temp dirs). No GPUI tests.

| Case | Expect |
|---|---|
| Warm, file present, size matches, SHA-1 known | Zero HTTP for that file; file body is not required to be hashed |
| Warm, size unknown, SHA-1 matches on disk | Zero HTTP; SHA-1 is computed once |
| Warm, size unknown, SHA-1 mismatches | Re-download |
| Warm, unhashed manifest mtime < 1h | Zero HTTP; parsed from disk |
| Warm, unhashed manifest mtime > 1h | HTTP replace |
| Warm, unhashed manifest stale, download fails, file parses | Use stale file |
| Warm, natives stamp present and matching | `extract_natives` not called |
| Warm, processor stamp matches installer SHA-1 | `run_one` not called |
| Verify, stamps present | Manifests fetched; SHA-1 runs; natives wiped and extracted; processors run |
| Verify | Caller does not `spawn`; engine does not spawn |
| Verify and Play overlap | `InstanceBusy` |
| Interrupted Verify (cancel after stamp delete) | Next Warm does not treat stamps as valid |

Existing prepare tests keep passing with `PrepareMode::Warm` (or `Verify` where they assert network).

## File touch list

| Path | Change |
|---|---|
| `crates/engine/src/lib.rs` | Export `PrepareMode` |
| `crates/engine/src/launch/mod.rs` | `prepare` takes mode; stop deleting manifests; pass mode down; native/processor stamps |
| `crates/engine/src/http.rs` | Mode-aware `cache_hit` |
| `crates/engine/src/java/mod.rs` | Stop deleting `java-all.json`; TTL |
| `crates/engine/src/mojang/libraries.rs` | Stamp-aware extract |
| `crates/engine/src/mojang/assets.rs` | Pass mode into downloads |
| `crates/engine/src/fabric/mod.rs`, `quilt/mod.rs` | Cache loader index with TTL |
| `crates/engine/src/forge/processors.rs` | Stamp skip |
| `src/app.rs`, `src/screens/instance_play.rs` | Verify button + prepare-without-spawn |
| `src/modals/progress.rs` | Title `Verifying files` when verifying |

Exact function names may shift; the mode and stamp contracts do not.

## Migration

No DB schema change. First Warm after upgrade extracts natives and runs processors once, then writes stamps. Users who never click Verify keep those stamps.
