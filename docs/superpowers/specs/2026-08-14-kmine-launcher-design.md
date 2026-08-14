# kmine Minecraft Launcher Design

Date: 2026-08-14  
Status: approved in conversation; awaiting file review

## Goal

A native Minecraft launcher written in Rust. The window is GPUI. The process starts the official game client (vanilla, Fabric, or Forge) with a Microsoft account. There is no in-app mod store and no modpack installer.

PandoraLauncher is an architecture reference only: look at it to see *in what order* a working launcher talks to Mojang / Microsoft / Fabric / Forge. Do not copy its source, types, message enums, file names, Seatbelt profile text, or Azure `CLIENT_ID`.

## Public protocols (not Pandora)

These are the sources of truth for on-the-wire formats:

- Microsoft identity platform: OAuth 2.0 authorization code + PKCE, consumers tenant
- Xbox Live user authenticate + XSTS (`rp://api.minecraftservices.com/`)
- `https://api.minecraftservices.com/authentication/login_with_xbox`
- `https://api.minecraftservices.com/minecraft/profile`
- Mojang `version_manifest_v2.json` and per-version JSON (modern `arguments` and legacy `minecraftArguments`)
- Mojang asset indexes (`virtual`, `map_to_resources`, object hash layout)
- Mojang Java runtime product JSON (`all.json` + per-runtime file manifest)
- Fabric meta (`/v2/versions/loader`, launch JSON)
- Forge installer JAR (install profile + processors)

## Scope

### In v1

- Microsoft accounts (multiple). Tokens live in SQLite, encrypted.
- Instances: create, rename, delete. Metadata in SQLite. Game files in `instances/<slug>/.minecraft/`.
- Loaders: vanilla, Fabric, Forge. Optional pinned loader version.
- Java: custom binary per instance, or download Mojang runtime for the version's `javaVersion.component`.
- Download and verify: client jar, libraries, natives, assets, log4j config.
- Play / Stop. Progress + cancel during prepare.
- Local content: list / enable / disable / delete files in `mods/`, `resourcepacks/`, `shaderpacks/`.
- Quick Play: world folder or server from the instance.
- Playtime and last played.
- Custom instance icon (PNG bytes in DB).
- Game output window. Redact access tokens before display.
- OS sandbox around the Java process (opt-in per instance).
- Persistence: single `kmine.db`. Master key in the OS keychain.

### Out of v1

- Modrinth / CurseForge browsers
- Modpack install or update (`.mrpack`, CurseForge zip, Pandora-style packs)
- Import from Prism / MultiMC / ATLauncher / official launcher
- Skin editor / 3D preview / skin library
- Cross-instance file sync
- NeoForge
- Offline / cracked accounts
- Launcher self-update
- Ads / accounts other than Microsoft

### Platforms

macOS is the first machine that must actually run the app (this repo is developed there). Windows and Linux compile in the same tree: same modules, `cfg`-gated keychain, sandbox, and Mojang Java platform id. If a sandbox backend cannot start (no `bwrap` on Linux), the instance checkbox is disabled and Play still works unsandboxed.

## Architecture

One OS process. Two runtimes:

- **UI thread** — GPUI. Draws windows. Must not `await` HTTP or hash multi-gigabyte trees on the UI thread.
- **Engine thread** — a multi-thread Tokio runtime. Auth, downloads, SQLite, process spawn.

They talk through a small owned event/command API. The UI does not import `kmine_engine::forge` or `kmine_engine::mojang`. It uses `Engine` plus events.

```
UI  --Command-->  Engine (tokio)
UI  <--Event----  Engine
```

Commands: `CreateInstance`, `RenameInstance`, `DeleteInstance`, `PrepareAndLaunch`, `KillInstance`, `StartLogin`, `SelectAccount`, `DeleteAccount`, `SetInstanceSettings`, `SetContentEnabled`, `DeleteContent`, `CancelPrepare`.

`StartLogin` is one shot: it binds the redirect port, opens the browser, waits for the callback, stores the account, then returns. There is no separate `FinishLogin`.

Events: `InstancesChanged`, `AccountsChanged`, `Progress { id, title, done, total }`, `PrepareFinished { id, result }`, `LogLine { instance_id, stream, text }`, `ProcessExited { instance_id, code }`, `AuthRequired`, `Error(EngineError)`.

Do not grow this into a 400-variant bridge. Add a variant when a screen needs it.

## Crate layout

Two packages. The current root `kmine` crate becomes a workspace member that holds the binary and GPUI. Logic moves to `crates/engine`.

```
kmine/                          # workspace root
  Cargo.toml                    # [workspace], members = [".", "crates/engine"]
  src/                          # GPUI binary
    main.rs                     # start tokio + gpui
    app.rs                      # window, Engine handle
    screens/
      instances.rs              # sidebar list
      instance_play.rs
      instance_content.rs
      instance_settings.rs
    modals/
      create_instance.rs
      accounts.rs
      progress.rs
    game_output.rs
    redact.rs
  crates/engine/
    src/
      lib.rs                    # Engine facade
      error.rs
      paths.rs
      http.rs                   # reqwest + sha1 download
      store/
        mod.rs
        migrate.rs
        crypto.rs
        keychain.rs
      auth/
      java/
      mojang/                   # manifest, version, rules, assets, libraries
      fabric/
      forge/
      instance/
      content.rs                # list/enable/disable/delete under .minecraft
      nbt.rs                    # servers.dat + level.dat LevelName
      launch/                   # LaunchPlan + argv substitution
      sandbox/
        mod.rs
        macos.rs
        linux.rs
        windows.rs
```

`crates/engine` has no `gpui` dependency.

## Engine facade

The UI holds an `EngineHandle` (cloneable, sends work onto the Tokio runtime). Snapshots are pull; changes are push via `Event`. No `Stream` on `Engine`.

```rust
pub struct Engine { /* store, http, paths, processes */ }

impl Engine {
    pub async fn open(paths: LauncherPaths) -> Result<Self, EngineError>;

    pub fn list_accounts(&self) -> Result<Vec<AccountSummary>, EngineError>;
    pub fn list_instances(&self) -> Result<Vec<InstanceSummary>, EngineError>;
    pub fn sandbox_status(&self) -> SandboxStatus;

    pub async fn create_instance(&self, spec: CreateInstance) -> Result<InstanceId, EngineError>;
    pub async fn rename_instance(&self, id: InstanceId, name: String) -> Result<(), EngineError>;
    pub async fn delete_instance(&self, id: InstanceId) -> Result<(), EngineError>;
    pub async fn update_instance(&self, id: InstanceId, patch: InstancePatch) -> Result<(), EngineError>;

    pub async fn start_login(&self) -> Result<AccountSummary, EngineError>;
    pub async fn select_account(&self, id: AccountId) -> Result<(), EngineError>;
    pub async fn delete_account(&self, id: AccountId) -> Result<(), EngineError>;

    pub fn list_content(&self, id: InstanceId, folder: ContentFolder) -> Result<Vec<ContentEntry>, EngineError>;
    pub fn set_content_enabled(&self, id: InstanceId, path: &Path, enabled: bool) -> Result<(), EngineError>;
    pub fn delete_content(&self, id: InstanceId, path: &Path) -> Result<(), EngineError>;
    pub fn list_quick_play(&self, id: InstanceId) -> Result<QuickPlayLists, EngineError>;

    pub async fn prepare(
        &self,
        id: InstanceId,
        progress: &dyn ProgressSink,
        cancel: CancellationToken,
        quick_play: Option<QuickPlay>,
    ) -> Result<LaunchPlan, EngineError>;

    pub fn spawn(&self, id: InstanceId, plan: LaunchPlan) -> Result<GameProcessId, EngineError>;
    pub fn kill(&self, id: InstanceId) -> Result<(), EngineError>;
}

pub struct CreateInstance {
    pub name: String,
    pub minecraft_version: String,
    pub loader: Loader,                 // Vanilla | Fabric | Forge
    pub loader_version: Option<String>,
    pub icon_png: Option<Vec<u8>>,
}

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

pub struct AccountSummary {
    pub uuid: AccountId,
    pub username: String,
    pub selected: bool,
}

pub struct InstanceSummary {
    pub id: InstanceId,
    pub slug: String,
    pub name: String,
    pub minecraft_version: String,
    pub loader: Loader,
    pub last_played_at: Option<i64>,
    pub playtime_secs: u64,
    pub running: bool,
}

pub enum ContentFolder { Mods, Resourcepacks, Shaderpacks }

pub struct ContentEntry {
    pub path: PathBuf,          // absolute, inside that folder
    pub name: String,           // file name without trailing .disabled
    pub enabled: bool,
}

pub enum QuickPlay {
    World { folder: String },
    Server { address: String },
}

pub struct QuickPlayLists {
    pub worlds: Vec<QuickPlayWorld>,
    pub servers: Vec<QuickPlayServer>,
}

pub struct QuickPlayWorld {
    pub folder: String,
    pub label: String,
}

pub struct QuickPlayServer {
    pub name: String,
    pub address: String,
}

pub enum SandboxStatus {
    Available,
    Unavailable { reason: String },
}

pub trait ProgressSink: Send + Sync {
    fn set(&self, title: &str, done: u64, total: u64);
}
```

`GameProcessId` is an opaque handle stored only inside `Engine`; the UI keys off `InstanceId`. `AccountId` and `InstanceId` are newtypes over `uuid::Uuid`.

`InstancePatch` fields wrapped in `Option` mean "leave unchanged"; `Some(None)` clears a nullable column.

## LaunchPlan

This is the spine. Every domain module only contributes fields. `spawn` does not re-fetch metadata.

```rust
pub struct LaunchPlan {
    pub java: PathBuf,
    pub jvm_args: Vec<String>,
    pub main_class: String,
    pub game_args: Vec<String>,
    pub classpath: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub cwd: PathBuf,                 // instances/<slug>/.minecraft
    pub env: Vec<(String, String)>,
    pub sandbox: SandboxSpec,
}

pub struct SandboxSpec {
    pub enabled: bool,
    pub allow_read: Vec<PathBuf>,
    pub allow_write: Vec<PathBuf>,
    pub network: bool,                // always true for Minecraft
}
```

Contribution table:

| Module | Writes |
|---|---|
| `auth` | substituted `${auth_player_name}`, `${auth_uuid}`, `${auth_access_token}`, `${user_type}` |
| `java` | `java` path |
| `mojang` | classpath entries, natives dir contents, asset args, vanilla main class + args, log4j |
| `fabric` / `forge` | extra libraries, replacement `main_class`, extra JVM/game args |
| `instance` | `-Xms`/`-Xmx`, extra JVM flags, `cwd`, sandbox enabled flag |
| `sandbox` | fills `allow_read` / `allow_write` from the finished plan |

## On-disk layout

Root (macOS): `~/Library/Application Support/kmine/`  
Windows: `%APPDATA%/kmine/`  
Linux: `$XDG_DATA_HOME/kmine` or `~/.local/share/kmine`

```
kmine/
  kmine.db
  instances/<slug>/.minecraft/     # vanilla game dir
  cache/meta/                      # version + loader JSON
  cache/libraries/
  cache/assets/indexes/
  cache/assets/objects/            # <ha>/<hash>
  cache/assets/virtual/legacy/     # only when index.virtual
  cache/runtime/<component>/<platform>/
  cache/natives/<plan-hash>/
```

No `config.json`, `accounts.json`, or `instance.json`.  
No Pandora directories: `contentlibrary`, `contentmeta`, `synced`, `sandbox` as a global game-home (Linux sandbox may still create a *temporary* XDG dir under `cache/` at spawn time).

`<slug>` is a unique filename-safe form of the display name (`My Pack` → `My Pack`, collision → `My Pack (2)`). `InstanceId` is a UUID and never changes when the name changes. Rename updates the DB row and renames the folder; if the folder rename fails, the DB transaction is not committed.

## SQLite (`kmine.db`)

Open with `rusqlite` (bundled). `PRAGMA journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000`. Schema version is `PRAGMA user_version`. Migrations live in `store/migrate.rs` and run in a single transaction per version bump.

### v1 schema

```sql
CREATE TABLE config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL          -- JSON text
);

CREATE TABLE accounts (
    uuid         TEXT PRIMARY KEY,   -- hyphenated Minecraft UUID
    username     TEXT NOT NULL,
    added_at     INTEGER NOT NULL,   -- unix ms
    last_used_at INTEGER
);

CREATE TABLE secrets (
    id         TEXT PRIMARY KEY,     -- 'account/<uuid>' | 'proxy-password'
    nonce      BLOB NOT NULL,        -- 12 bytes
    ciphertext BLOB NOT NULL         -- AES-256-GCM including tag
);

CREATE TABLE instances (
    id               TEXT PRIMARY KEY,  -- hyphenated UUID
    slug             TEXT NOT NULL UNIQUE,
    name             TEXT NOT NULL,
    minecraft_version TEXT NOT NULL,
    loader           TEXT NOT NULL,     -- 'vanilla' | 'fabric' | 'forge'
    loader_version   TEXT,              -- null = latest compatible at prepare
    account_uuid     TEXT,              -- null = use config.selected_account
    memory_min_mb    INTEGER,           -- null = do not pass -Xms
    memory_max_mb    INTEGER,           -- null = do not pass -Xmx
    jvm_flags        TEXT,              -- extra flags, shell-split
    java_path        TEXT,              -- null = Mojang runtime
    sandbox          INTEGER NOT NULL DEFAULT 0,
    icon_png         BLOB,
    created_at       INTEGER NOT NULL,
    last_played_at   INTEGER,
    playtime_secs    INTEGER NOT NULL DEFAULT 0,
    session_count    INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (account_uuid) REFERENCES accounts(uuid) ON DELETE SET NULL
);
```

`config` keys in v1:

| key | value |
|---|---|
| `selected_account` | JSON string UUID or `null` |
| `window` | `{"width":n,"height":n}` |

Do not store tokens in `config` or `accounts`.

### Secrets encryption

- Algorithm: AES-256-GCM.
- Master key: 32 random bytes, generated once, stored only in the OS keychain.
- Keychain service: `dev.kmine.launcher`, account: `master-key`.
  - macOS: Security.framework generic password
  - Windows: Credential Manager generic credential
  - Linux: Secret Service via the `oo7` crate
- AAD for each seal: the UTF-8 `secrets.id` bytes. Opening with a different id must fail.
- Payload for `account/<uuid>` is JSON:

```json
{
  "msa_refresh": "...",
  "msa_access": {"token":"...","expiry":"..."},
  "xbl": {"token":"...","expiry":"..."},
  "xsts": {"token":"...","expiry":"...","userhash":"..."},
  "mc_access": {"token":"...","expiry":"..."}
}
```

Refresh from the newest still-valid stage (mc access → xsts → xbl → msa access → msa refresh). If refresh returns `invalid_grant`, delete that secret row and emit `AuthExpired`.

If the master key is missing (new machine, wiped keychain), existing `secrets` rows are unreadable. Leave the rows, surface `AuthExpired` for those accounts, and generate a new master key so future logins work.

## Auth

Own Azure App registration (consumers). Redirect URI registered exactly as:

`http://127.0.0.1:47821/auth`

`CLIENT_ID` is a `&str` constant in `crates/engine/src/auth/constants.rs`. It is not Pandora's id. If the constant is empty, `start_login` returns `EngineError::AuthNotConfigured` and the Accounts modal tells the user to set the Azure client id.

Flow:

1. PKCE S256, scopes `XboxLive.signin` and `XboxLive.offline_access`, `prompt=select_account`.
2. Bind `127.0.0.1:47821`, open the authorize URL in the system browser.
3. Validate `state`, exchange the code.
4. Xbox user authenticate (`RPS` / `d=<msa_access>`).
5. XSTS for `rp://api.minecraftservices.com/`.
6. `login_with_xbox` → Minecraft access token.
7. `GET /minecraft/profile` → uuid + username. No profile (no game owned) is `EngineError::MinecraftNotOwned`.
8. Upsert `accounts` and seal `secrets`.

Login is single-flight (one semaphore). A second "Add account" waits or is rejected with `EngineError::LoginInProgress`.

## prepare() pipeline

`prepare` is cancelable. On cancel, in-flight downloads abort and no `LaunchPlan` is returned.

1. Load instance row. Resolve account: instance `account_uuid` or `config.selected_account`. Missing account → `AuthRequired` / `NoAccount`.
2. Ensure a live Minecraft access token (refresh chain above).
3. Fetch `version_manifest_v2` (cached in `cache/meta/` with ETag or sha1 when the manifest link has one). Resolve `minecraft_version`.
4. Fetch that version JSON. Verify sha1 when present.
5. Loader:
   - **vanilla** — version JSON as-is.
   - **Fabric** — Fabric loader manifest; use `loader_version` or latest stable; fetch launch JSON; append loader/intermediary/common/client libraries; replace `mainClass`.
   - **Forge** — Forge maven/installer for that Minecraft version; use `loader_version` or newest matching installer; run installer processors; merge libraries and main class. Processors run unsandboxed (they are the launcher, not the game).
6. Java: if `java_path` is set, resolve a `java` binary under it (the path itself, or `bin/java`, or macOS `Contents/Home/bin/java`). Else map `(OS, ARCH)` to Mojang platform id (`mac-os-arm64`, `mac-os`, `linux`, `windows-x64`, …). On `mac-os-arm64` with a missing component, fall back to `mac-os` (Rosetta). Download the runtime file manifest and files into `cache/runtime/<component>/<platform>/`. Missing/wrong sha1 → re-download that file only.
7. In parallel: client jar, libraries whose rules allow this OS/arch, native classifiers/extract (legacy) or modern native jars, asset objects, log4j xml. Shared objects live under `cache/assets/objects/<2-hex>/<sha1>`. If `map_to_resources`, copy/link into `.minecraft/resources`. If `virtual`, materialize under `cache/assets/virtual/legacy`.
8. Extract natives into `cache/natives/<hash>/` where `<hash>` is a digest of the native artifact set (and `-sandbox` suffix when sandbox is on, so extract layouts cannot clash).
9. Build argv. Support both `arguments.jvm` / `arguments.game` (ruled) and legacy `minecraftArguments`. Substitute at least:

   `${auth_player_name}`, `${auth_uuid}`, `${auth_access_token}`, `${user_type}`, `${version_name}`, `${game_directory}`, `${assets_root}`, `${assets_index_name}`, `${natives_directory}`, `${launcher_name}`, `${launcher_version}`, `${classpath}`, `${resolution_width}`, `${resolution_height}`, `${quickPlayPath}`, `${quickPlaySingleplayer}`, `${quickPlayMultiplayer}`.

   Unknown placeholders: leave untouched only if the official spec says so; otherwise drop the ruled argument that requires an unused feature (`is_demo_user`, quick play variants not requested).
10. Apply instance memory and `jvm_flags`.
11. Fill `SandboxSpec` from the finished paths. Return `LaunchPlan`.

Cache hit rule: local file exists and sha1 matches. Missing sha1 in the artifact (some Fabric libs): use size + exists, and re-download if the server sends a different `content-length` on a conditional get; do not silently accept an empty file.

## spawn() and sandbox

`spawn` starts `java` with `LaunchPlan` working directory `cwd`. The engine keeps the child, forwards stdout/stderr as `LogLine` after `redact`, and on exit updates `playtime_secs`, `session_count`, `last_played_at`.

Redact before any UI or log file: Minecraft access tokens, MSA tokens, and query values that look like `accessToken=...`.

If `sandbox.enabled` is false: `std::process::Command` (or a thin wrapper that still captures pipes).

If true:

| OS | Mechanism |
|---|---|
| macOS | Seatbelt profile via `sandbox_init` / `sandbox_init_with_parameters` |
| Linux | `bwrap` + a tight seccomp filter. If `bwrap` is not on `PATH`, spawn returns `SandboxUnavailable` only when the user demanded sandbox; the checkbox is disabled beforehand via `Engine::sandbox_status()` |
| Windows | AppContainer. Grant read/write ACLs on `allow_*` paths. Network capabilities on. |

Whitelist:

- **write:** `cwd` (`.minecraft`), `natives_dir`, and on Linux a per-spawn XDG dir under `cache/`
- **read:** Java home, `cache/libraries`, `cache/assets`, `cache/runtime`, the `java` binary
- **network:** yes
- **GPU / audio / window:** yes (device binds / Seatbelt graphics / AppContainer defaults as required to actually show a window)

Deny: home directory, other instance folders, `kmine.db`, keychain.

Write the Seatbelt/bwrap/AppContainer policy in-tree from vendor documentation. Do not paste Pandora's profile strings.

Default `instances.sandbox = 0`. Settings UI shows a warning that native mods and RPC may break.

## UI (GPUI)

Stack: `gpui` + `gpui_platform` + `gpui-component` already in the root `Cargo.toml`. Dark theme. Not a visual clone of Pandora.

One main window:

```
┌─────────────┬──────────────────────────────┐
│ instances   │  selected instance           │
│ + create    │  Play / Stop                 │
│             │  tabs: Play | Content |      │
│             │  Settings                    │
├─────────────┴──────────────────────────────┤
│ account (username)     prepare status      │
└────────────────────────────────────────────┘
```

| Surface | Behavior |
|---|---|
| Sidebar | Rows from `instances`: name, `minecraft_version`, loader, last played. Selection is UI state, not a DB column. |
| Create modal | Name, Minecraft version (from cached manifest; fetch on open), loader, optional loader version. Creates DB row + empty `.minecraft`. |
| Play tab | Play/Stop, playtime, last played, Quick Play. Worlds: each subdirectory of `saves/` that contains `level.dat`. Label is NBT `Data.LevelName` if present, else the folder name. Servers: parse `.minecraft/servers.dat` list (`name`, `ip`) in `nbt.rs`. |
| Content tab | Files in `mods/`, `resourcepacks/`, `shaderpacks/` (non-recursive, one level). A file is disabled iff its name ends with `.disabled` (`sodium.jar` ↔ `sodium.jar.disabled`). Enable/disable is `rename`. Delete is `remove_file`. Ignore dotfiles. No download UI. |
| Settings tab | RAM sliders, JVM flags text, Java path picker, sandbox checkbox + warning (disabled when `sandbox_status` is `Unavailable`), per-instance account dropdown. |
| Accounts modal | List, select (writes `config.selected_account`), delete (row + secret), Add account starts OAuth. |
| Progress modal | Title + counts from `ProgressSink`. Cancel triggers the prepare `CancellationToken`. |
| Game output | Second window, attached to a running process. Closed window does not kill the game. |

Play on an instance that is already preparing or running is ignored. The sidebar stays interactive during prepare.

`launcher_name` in argv is `kmine`. `launcher_version` is `CARGO_PKG_VERSION`.

## Errors

`EngineError` is the only type the UI matches on:

- `AuthNotConfigured`
- `AuthExpired`
- `AuthFailed { message }`
- `MinecraftNotOwned`
- `LoginInProgress`
- `NoAccount`
- `VersionNotFound { id }`
- `LoaderUnavailable { loader, minecraft }`
- `ChecksumMismatch { path, expected, actual }`
- `JavaNotFound`
- `SandboxUnavailable { reason }`
- `Cancelled`
- `InstanceBusy`
- `Io { path, source }`
- `Sqlite { source }`
- `Crypto`
- `Http { url, status }`

No panics across the UI/engine boundary. A panic on the tokio worker is caught, logged, and emitted as `Error`.

## Testing

Engine tests do not start GPUI.

| Area | How |
|---|---|
| `store` | tempfile DB + in-memory keychain fake. Round-trip config, accounts, seal/open, AAD mismatch fails, migrate v0→v1. |
| `mojang` | Checked-in fixture JSON (minimal 1.20.4-style `arguments`, plus a 1.12.2-style `minecraftArguments`). Rule eval for OS/arch. Arg substitution. |
| `fabric` / `forge` | Fixtures that change `main_class` and append libraries. |
| `auth` | `mockito`/`wiremock` HTTP. Refresh `invalid_grant` → `AuthExpired`. |
| `launch` | Build a `LaunchPlan` from fixtures; assert sandbox write set is only `.minecraft` + natives. |
| `nbt` | Fixture `servers.dat` and `level.dat` bytes; parse name/ip and `LevelName`. |
| Live Mojang | `#[ignore]` integration tests, not CI-default. |

UI is verified by running the binary. There is no GPUI screenshot suite in v1.

## Global constraints

- Rust edition **2024** (already set).
- Workspace: `kmine` + `crates/engine`.
- Database file name is exactly `kmine.db`.
- OAuth bind: `127.0.0.1:47821` / path `/auth`.
- Keychain: service `dev.kmine.launcher`, account `master-key`.
- Tokens never written plaintext to disk or logs.
- Own Microsoft `CLIENT_ID` only.
- Do not vendor or copy PandoraLauncher source.
- YAGNI: no extra crates (`schema`, `bridge`, `auth` as their own package) until a second binary needs them.

## Implementation order (for the later plan)

1. Workspace split + `LauncherPaths` + empty `Engine::open`.
2. `store` (migrate, crypto, keychain fake + real).
3. Instance CRUD + empty `.minecraft` (enough to drive the sidebar).
4. GPUI shell: list, create modal, settings that only touch SQLite.
5. Mojang manifest + version JSON + rules + argv (vanilla, no download yet) against fixtures.
6. `http` download + sha1 + libraries/assets/client.
7. Java runtime install.
8. Auth + Accounts modal.
9. `prepare` + `spawn` vanilla Play + game output + redact.
10. Fabric.
11. Forge.
12. Content tab + Quick Play.
13. Sandbox backends + checkbox.

Each step leaves `cargo test -p kmine-engine` green.

## Key decisions

1. **From scratch, Pandora as a map.** Avoid inheriting their crate soup and out-of-scope features. Cost: we own bugs in rules, natives, and Forge processors; mitigated with fixtures.
2. **Two crates, `LaunchPlan` as the spine.** UI stays thin; launch stages stay testable without windows.
3. **SQLite `kmine.db` + one keychain secret.** Queryable state, restorable DB file that is useless without the master key.
4. **Sandbox is opt-in spawn wrapping.** Prepare stays privileged; the game is what we jail.
5. **No store, no packs in v1.** Local files only. Keeps the first shippable Play path finite.
6. **macOS first, same tree for Win/Linux.** Feature flags, not forks.
