# kmine

A native Minecraft launcher. One process. Two runtimes. Zero Electron.

kmine is written in Rust. The window is [GPUI](https://github.com/zed-industries/zed) — the same GPU UI Zed uses. The game is the official client: vanilla, Fabric, Forge, NeoForge, or Quilt, started with a real Microsoft account.

It is built like a desktop product, not like a browser that happens to launch Java.

---

## Why this exists

Most launchers are either Electron shells, Java UIs, or kitchen-sink mod stores with ads. kmine is the opposite: a small native app whose job is to prepare the game, keep your tokens off disk in plaintext, and get out of the way.

What that buys you:

| You get | Instead of |
|---|---|
| A GPU-composited native window (macOS blur, Windows Mica) | A Chromium process that weighs more than the game |
| Microsoft OAuth with PKCE | A pasted token, a cracked offline slot, or a mystery login webview |
| AES-256-GCM secrets, master key in the OS keychain | Tokens sitting in a JSON file next to your worlds |
| An opt-in OS sandbox around Java | Mods that can read your whole home directory because they felt like it |
| SHA-1 verified, cancellable, concurrent downloads | A progress bar that lies and a half-written jar |
| Access tokens and JWTs stripped from the game log | Your session key in a screenshot |
| A catalog the engine owns, not the UI | A second app that is just a CurseForge browser |

---

## What it does

### Play the game, properly

- **Microsoft accounts**, several of them. Sign-in is one shot: bind a loopback port, open the browser, PKCE callback, Xbox Live → XSTS → `minecraftservices`, done. No “finish login” step.
- **Instances** with their own `.minecraft`, icon, memory, JVM flags, Java binary, and sandbox switch.
- **Loaders:** vanilla, Fabric, Forge, NeoForge, Quilt. Pin a loader version, or let kmine pick.
- **Java:** a custom binary per instance, or the Mojang runtime that version actually asks for.
- **Prepare then Play.** Client jar, libraries, natives, assets, log4j config — downloaded, hashed, cancellable. Then spawn. Then Stop.
- **Quick Play** into a world or a server. Worlds come from `level.dat`. Servers come from `servers.dat`. Both are parsed as NBT, not guessed.
- **Playtime and last played**, stored, not estimated from “the window was open”.

### Own your content

- List, enable, disable, delete files in `mods/`, `resourcepacks/`, `shaderpacks/`. Disable is a `.disabled` suffix, the same convention everyone already uses.
- **Catalog** for CurseForge: search, categories, project, file, install. Same modal creates a pack as a new instance, or adds a mod / pack / shader to the one you already have.
- **Required dependencies** install with the file. Optional ones stay optional — no surprise junk.
- **Pack install rolls back.** Cancel or fail, and the half-built instance is deleted. Adding content to an existing instance keeps what already landed and stops on the first error.

A later store (Modrinth, anyone) is a second `CatalogProvider`, not a second UI.

### Stay a desktop citizen

- Warm charcoal UI, not a default-component dump. Primary actions are a white pill. Running is moss, not neon.
- Glass sidebar on macOS and Windows. Smooth scrolling. Instance pins. Skin face in the chrome.
- A dedicated **game output** window: stdout / stderr, log levels, 4000-line ring. Tokens never make it onto the screen.
- macOS first (this tree is developed there). Windows and Linux compile from the same modules — keychain, sandbox, and Mojang Java platform ids are `cfg`-gated, not forked.

---

## Security, for real

Launchers handle the one secret that can take your Minecraft account. kmine treats that as the product, not a footnote.

**Accounts**

- OAuth 2.0 authorization code + PKCE against the Microsoft consumers tenant.
- CSRF `state` is checked. The callback times out. Login is a mutex — you cannot start two at once.
- Refresh lives in SQLite, **encrypted**. The 32-byte master key never sits next to the ciphertext: it lives in the OS keychain (`dev.kmine.launcher`).
  - macOS: Security.framework
  - Windows: Credential Manager
  - Linux: Secret Service (`oo7`)
- AES-256-GCM with the secret id as AAD. Open the blob under the wrong id and it fails closed.

**Process**

- Opt-in **sandbox per instance**. Off by default so Play still works; on, Java is jailed.
  - **macOS** — Seatbelt, deny-default, authored against `sandbox_init(3)` and `system.sb`. WindowServer, HID, audio, and LWJGL native mapping are allowed. Your `kmine.db` is not writable.
  - **Linux** — `bwrap --unshare-all`, network shared only because the game needs it. Wayland / Pulse / PipeWire sockets bind; the session bus and keyring do not. No `bwrap` on `PATH` → the checkbox disables, Play still works unsandboxed.
  - **Windows** — AppContainer profile, capability SIDs, DACLs, `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`.
- Temp, JNA, LWJGL extract, and Netty native dirs are forced into the writable set so the jail does not just crash the game.
- Download paths go through `safe_join`. `..`, absolute paths, and empty segments are rejected. Catalog cache blobs are sanitized the same way.

**Logs and keys**

- Game output redacts exact tokens, `accessToken=…`, and JWTs before a line is shown.
- The CurseForge Core `x-api-key` is **not** baked into the launcher and is **not** extracted there. A separate `kmine-backend-api` process holds it in memory and serves `GET /get_cf_api_key`. The launcher refreshes hourly, stores the last good key encrypted, and never prints it. `Client`’s `Debug` impl is non-exhaustive for the same reason.

**Transport**

- `reqwest` + **rustls**. No OpenSSL in the tree.
- SHA-1 check when Mojang or CurseForge gave a hash. Retries with backoff. Cancel is a first-class token, not “close the window and hope”.

---

## Architecture

One OS process. The UI thread must not `await` HTTP or hash a multi-gigabyte asset tree.

```
┌──────────────────────────────────────────────────────────┐
│  kmine  (GPUI)                                           │
│  screens · modals · chrome · CurseForgeProvider adapter  │
└───────────────────────┬──────────────────────────────────┘
                        │  EngineHandle  (commands / events)
┌───────────────────────▼──────────────────────────────────┐
│  kmine-engine  (Tokio, no gpui)                          │
│  auth · mojang · fabric · forge · neoforge · quilt       │
│  java · launch · sandbox · store · catalog · nbt         │
└───────────┬───────────────────────────────┬──────────────┘
            │                               │
            ▼                               ▼
     Microsoft / Xbox /               ┌─────────────────────┐
     Mojang / loader meta             │ kmine-backend-api   │
                                      │ GET /get_cf_api_key │
                                      └──────────┬──────────┘
                                                 │
            ┌────────────────────────────────────┘
            ▼
     kmine-curseforge  (bytes only — never writes a path)
            │
            ▼
     api.curseforge.com · ForgeCDN
```

The binary talks to the engine. The engine talks to the world. The UI never imports `kmine_engine::forge` or CurseForge wire types.

`kmine-curseforge` is a Minecraft-only Core client: search, categories, projects, files, fingerprints (whitespace-stripped MurmurHash2), pack `manifest.json`, in-memory override walk. Downloads return `bytes::Bytes`. Disk is the operator’s problem.

`kmine-engine` does **not** depend on `kmine-curseforge`. The adapter lives in the binary. That is how a second provider gets in without a rewrite.

Persistence is one file: `kmine.db`. Schema is versioned. Bundled SQLite.

---

## Status

**0.1.0.** macOS is the machine that has to actually run. Windows and Linux are in the same tree and compile; treat them as real ports, not brochures.

In:

- Microsoft login, instances, five loaders, Java, prepare / play / stop
- Local content, catalog (CurseForge), pack install with rollback
- Sandbox, log redaction, keychain, catalog key refresh

Not in, on purpose:

- Modrinth (the trait is ready; the adapter is not)
- Pack / mod updates, changelogs, pin-to-latest
- Import from Prism / MultiMC / ATLauncher / the official launcher
- Offline or cracked accounts
- Launcher self-update
- Ads

There are **200+** unit and integration tests — OAuth against wiremock, SHA-1 downloads, Forge processors, Seatbelt profile text, bwrap socket policy, catalog rollback, path escape, AES-GCM AAD, NBT, log redaction. GPUI is not unit-tested. That is a choice, not a claim that the window cannot break.

---

## Build

Needs a current stable Rust (**edition 2024**, this repo is developed on rustc 1.95).

```bash
git clone https://github.com/gfhfyjbr/kmine.git
cd kmine
cargo run --release
```

Release is `thin` LTO, one codegen unit, stripped, `panic = abort`. Debug is fine for UI work.

Tests:

```bash
cargo test --workspace
```

### Catalog (optional)

The catalog is silent until a Core key exists. Run the backend against an official CurseForge build (app bundle, asar, or DMG), then start kmine:

```bash
export KMINE_CF_KEY_SOURCE=/path/to/CurseForge.app   # or a .dmg / .asar
export KMINE_BACKEND_TOKEN=dev
cargo run -p kmine-backend-api

# other terminal
export KMINE_BACKEND_URL=http://127.0.0.1:8787
export KMINE_BACKEND_TOKEN=dev
cargo run --release
```

`GET /get_cf_api_key` is the only route that matters. On Unix, `SIGHUP` re-extracts. The key is never logged.

There is also `cf-key` in `kmine-curseforge` if you just want to see that extraction works.

---

## Environment

| Variable | Default | Who |
|---|---|---|
| `KMINE_MSA_CLIENT_ID` | baked Azure app | launcher — override for local login tests |
| `KMINE_MSA_REDIRECT_URL` | `http://127.0.0.1:47821/auth` | launcher — must match that Azure app |
| `KMINE_MSA_BIND` | host:port from the redirect | launcher |
| `KMINE_BACKEND_URL` | `http://127.0.0.1:8787` | launcher |
| `KMINE_BACKEND_TOKEN` | unset | launcher **and** backend — Bearer if set |
| `KMINE_DOWNLOAD_CONCURRENCY` | `64` (max `128`) | engine |
| `KMINE_BACKEND_BIND` | `127.0.0.1:8787` | backend |
| `KMINE_CF_KEY_SOURCE` | unset | backend — path or `http(s)://…` |

Data lives under the platform data dir, `kmine/`:

- macOS: `~/Library/Application Support/kmine`
- Linux: `~/.local/share/kmine`
- Windows: `%APPDATA%\kmine`

```
kmine/
  kmine.db
  instances/<slug>/.minecraft/
  cache/
    meta/  libraries/  assets/  runtime/  natives/  skins/
    catalog/files/  catalog/images/
```

---

## Workspace

```
kmine/                      GPUI binary — window, screens, provider adapter
crates/engine/              launcher brain — no gpui
crates/curseforge/          CurseForge Core client + official-app key extract
crates/backend-api/         tiny Axum process that serves the Core key
```

Read the specs if you want the decisions, not just the shape:

- [`docs/superpowers/specs/2026-08-14-kmine-launcher-design.md`](docs/superpowers/specs/2026-08-14-kmine-launcher-design.md)
- [`docs/superpowers/specs/2026-08-15-catalog-integration-design.md`](docs/superpowers/specs/2026-08-15-catalog-integration-design.md)
- [`docs/superpowers/specs/2026-08-15-curseforge-crate-design.md`](docs/superpowers/specs/2026-08-15-curseforge-crate-design.md)

Public wire formats (Mojang, Microsoft, Fabric, Forge, CurseForge Core) are the source of truth. Other launchers were an order-of-operations reference. Their source, types, and Azure `CLIENT_ID` are not.

---

## License

Not declared yet. Treat the tree as all rights reserved until it is.
