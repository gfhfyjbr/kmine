# kmine-curseforge Core Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the async Minecraft CurseForge Core client in `crates/curseforge` (`kmine-curseforge`): search, categories, projects, files, fingerprints, pack manifests, and file bytes — never writing to disk.

**Architecture:** Same crate as the existing key extractor. New modules (`error`, `types`, `search`, `client`, `download`, `manifest`, `fingerprint`) sit next to unchanged `extract`/`asar`/`dmg`/`fetch`/`cf-key`. `Client` owns a `reqwest::Client` plus a caller-supplied `x-api-key`. Callers provide Tokio. Tests use wiremock; default `cargo test` never hits `api.curseforge.com`.

**Tech Stack:** Rust edition 2024, reqwest 0.12 (rustls + json + stream + blocking), serde/serde_json, bytes, sha1, hex, zip 8, thiserror, wiremock 0.6 (dev), tokio (dev).

**Spec:** `docs/superpowers/specs/2026-08-15-curseforge-crate-design.md`

## Global Constraints

- Package name stays `kmine-curseforge`. Workspace member already listed.
- Do not depend on `kmine-engine`. Do not import this crate from `kmine` or `kmine-engine`.
- Do not bake the Overwolf Core key into source. Caller passes the key.
- The crate never creates, opens, or writes a filesystem path (existing extract/`cf-key` may read; new catalog code must not).
- Minecraft only: `gameId` is always constant `432`, never a method argument.
- No CF Bearer, SSO, `app-search`, share-codes, comments, favorites, highlights.
- `reqwest` stays rustls. Keep `blocking` for `cf-key`. Add `json` and `stream`.
- Do not add `chrono`, `murmur3`, or `kmine-engine`. Implement MurmurHash2 in-tree.
- `Client` is `Clone + Send + Sync`. `Debug` for `Client` must not contain the API key. `Error` display must not contain the API key.
- JSON headers (`x-api-key`, `Accept: application/json`) only on `api.curseforge.com`. CDN downloads send User-Agent only.
- Array query params are JSON array strings (`[4736,4475]`, `["1.20.1"]`), not CSV.
- Search/list `pageSize` 1..=50, `index + pageSize <= 10000`, default `pageSize` 20. Batch chunk size 100.
- v2 where the official app uses v2: `GET /v2/mods/search`, `GET /v2/mods/{id}`, `POST /v2/mods/get-mods-by-ids`.
- No live `api.curseforge.com` in default `cargo test`.
- Existing extract tests stay green: `pulls_key_from_asar_js` and friends.
- After every task: `cargo test -p kmine-curseforge` is green.
- Rust edition **2024**.

## File structure

| Path | Responsibility |
|---|---|
| `crates/curseforge/Cargo.toml` | deps: bytes, sha1, hex; reqwest features; dev tokio + wiremock |
| `crates/curseforge/src/lib.rs` | modules + re-exports; crate docs |
| `crates/curseforge/src/error.rs` | `Error`, `ResourceKind` |
| `crates/curseforge/src/types.rs` | constants, `ClassId`, enums, `Category`/`Mod`/`File`/…, helpers |
| `crates/curseforge/src/search.rs` | `SearchQuery`, `FileFilter`, `CategoryFilter` |
| `crates/curseforge/src/client.rs` | `Client`, `ClientBuilder`, transport, retries, all HTTP methods |
| `crates/curseforge/src/download.rs` | `Downloaded`, `cdn_file_url`, `resolve_download_url` |
| `crates/curseforge/src/manifest.rs` | `Manifest`, `PackZip`, `PackOverride`, `ResolvedPack` |
| `crates/curseforge/src/fingerprint.rs` | `fingerprint()`, `FingerprintMatches`, `FingerprintMatch` |
| `crates/curseforge/src/asar.rs` | existing — do not change |
| `crates/curseforge/src/dmg.rs` | existing — do not change |
| `crates/curseforge/src/extract.rs` | existing — do not change |
| `crates/curseforge/src/fetch.rs` | existing — do not change |
| `crates/curseforge/src/bin/cf-key.rs` | existing — do not change |
| `crates/curseforge/tests/fixtures/mod_jei.json` | JEI-shaped `Mod` envelope `data` |
| `crates/curseforge/tests/fixtures/file_5754631.json` | File 5754631 from the reverse doc |
| `crates/curseforge/tests/fixtures/manifest_sf5.json` | SkyFactory-shaped pack manifest |

---

### Task 1: Error, types, fixtures, helpers

**Files:**
- Modify: `crates/curseforge/Cargo.toml`
- Modify: `crates/curseforge/src/lib.rs`
- Create: `crates/curseforge/src/error.rs`
- Create: `crates/curseforge/src/types.rs`
- Create: `crates/curseforge/tests/fixtures/mod_jei.json`
- Create: `crates/curseforge/tests/fixtures/file_5754631.json`
- Test: `crates/curseforge/src/types.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: nothing new
- Produces:
  - `Error` and `ResourceKind` as specified below
  - `MINECRAFT_GAME_ID: u32 = 432`
  - `DEFAULT_BASE_URL: &str = "https://api.curseforge.com"`
  - `DEFAULT_PAGE_SIZE: u32 = 20`
  - `MAX_PAGE_SIZE: u32 = 50`
  - `MAX_INDEX_PLUS_PAGE: u32 = 10_000`
  - `BATCH_SIZE: usize = 100`
  - `ClassId(pub u32)` with the nine associated constants
  - enums `ModLoaderType`, `SortField`, `SortOrder`, `FileReleaseType`, `FileRelationType`, `HashAlgo`
  - structs `Page<T>`, `Pagination`, `Category`, `Mod`, `ModLinks`, `ModAuthor`, `ModAsset`, `FileIndex`, `File`, `FileHash`, `FileDependency`, `FileModule`, `SortableGameVersion`, `MinecraftVersion`, `ModLoaderIndexEntry`, `ModLoaderInfo`
  - `Pagination::next_index(&self) -> Option<u32>`
  - `File::sha1(&self) -> Option<&str>`
  - `File::md5(&self) -> Option<&str>`
  - `File::is_approved(&self) -> bool`
  - `File::required_mod_ids(&self) -> impl Iterator<Item = u32> + '_`
  - `FileIndex::matches(&self, mc: &str, loader: Option<ModLoaderType>) -> bool`
  - `Mod::file_index_for(&self, mc: &str, loader: Option<ModLoaderType>) -> Option<&FileIndex>`

- [ ] **Step 1: Add crate deps**

Replace `crates/curseforge/Cargo.toml` with:

```toml
[package]
name = "kmine-curseforge"
version.workspace = true
edition.workspace = true

[dependencies]
bytes = "1"
hex = "0.4"
regex = "1"
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls", "stream"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha1 = "0.10"
thiserror = "2"
udif = "0.3"
zip = { version = "8", default-features = false, features = ["deflate"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
wiremock = "0.6"
```

Do not add `kmine-engine`, `chrono`, or a murmur crate.

- [ ] **Step 2: Write fixtures**

`crates/curseforge/tests/fixtures/mod_jei.json`:

```json
{
  "id": 238222,
  "gameId": 432,
  "name": "Just Enough Items (JEI)",
  "slug": "jei",
  "links": {
    "websiteUrl": "https://www.curseforge.com/minecraft/mc-mods/jei",
    "wikiUrl": "",
    "issuesUrl": "https://github.com/mezz/JustEnoughItems",
    "sourceUrl": "https://github.com/mezz/JustEnoughItems"
  },
  "summary": "Item and recipe viewing mod",
  "status": 4,
  "downloadCount": 123456789,
  "isFeatured": false,
  "primaryCategoryId": 421,
  "categories": [
    {
      "id": 421,
      "gameId": 432,
      "name": "API and Library",
      "slug": "library-api",
      "isClass": false,
      "classId": 6
    }
  ],
  "classId": 6,
  "authors": [{ "id": 1, "name": "mezz", "url": "https://www.curseforge.com/members/mezz" }],
  "logo": {
    "id": 1,
    "modId": 238222,
    "thumbnailUrl": "https://media.forgecdn.net/avatars/thumbnails/jei.png",
    "url": "https://media.forgecdn.net/avatars/jei.png"
  },
  "screenshots": [],
  "mainFileId": 5700000,
  "latestFiles": [],
  "latestFilesIndexes": [
    {
      "gameVersion": "1.20.1",
      "fileId": 5700000,
      "filename": "jei-1.20.1-xxx.jar",
      "releaseType": 1,
      "gameVersionTypeId": 1,
      "modLoader": 1
    }
  ],
  "dateCreated": "2015-11-19T00:00:00Z",
  "dateModified": "2025-01-01T00:00:00Z",
  "dateReleased": "2025-01-01T00:00:00Z",
  "allowModDistribution": true,
  "gamePopularityRank": 1,
  "isAvailable": true,
  "thumbsUpCount": 0
}
```

`crates/curseforge/tests/fixtures/file_5754631.json`:

```json
{
  "id": 5754631,
  "gameId": 432,
  "modId": 250898,
  "isAvailable": true,
  "displayName": "oreexcavation-1.13.174.jar",
  "fileName": "oreexcavation-1.13.174.jar",
  "releaseType": 1,
  "fileStatus": 4,
  "hashes": [
    { "algo": 1, "value": "19b1540f5e69fe6d04d174915e834bb614bf51ce" },
    { "algo": 2, "value": "1e87b83ed930e864de2a3150255f30bf" }
  ],
  "fileDate": "2024-09-25T09:04:51.11Z",
  "fileLength": 277361,
  "downloadCount": 0,
  "downloadUrl": "https://edge.forgecdn.net/files/5754/631/oreexcavation-1.13.174.jar",
  "gameVersions": ["1.20.1", "Forge"],
  "sortableGameVersions": [
    {
      "gameVersionName": "1.20.1",
      "gameVersionPadded": "0000000001.0000000020.0000000001",
      "gameVersion": "1.20.1",
      "gameVersionReleaseDate": "2023-06-07T00:00:00Z",
      "gameVersionTypeId": 1
    }
  ],
  "dependencies": [{ "modId": 123456, "relationType": 3 }],
  "alternateFileId": 0,
  "isServerPack": false,
  "serverPackFileId": 0,
  "isEarlyAccessContent": false,
  "fileFingerprint": 3871571640,
  "modules": [{ "name": "oreexcavation", "fingerprint": 1095884136 }]
}
```

- [ ] **Step 3: Write the failing tests**

Create `crates/curseforge/src/types.rs` with only the tests first (the module will not compile until types exist — that is the fail). Prefer putting tests at the bottom and types above in the implement step. For TDD, write this test module now and a stub `types.rs` that does not yet deserialize the fixtures.

Put the tests in `crates/curseforge/src/types.rs` under `#[cfg(test)] mod tests`. They will not compile until the types exist; that counts as the failing red. After types exist, run them.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn load_mod() -> Mod {
        serde_json::from_str(include_str!("../../tests/fixtures/mod_jei.json")).unwrap()
    }

    fn load_file() -> File {
        serde_json::from_str(include_str!("../../tests/fixtures/file_5754631.json")).unwrap()
    }

    #[test]
    fn jei_mod_fixture() {
        let m = load_mod();
        assert_eq!(m.id, 238222);
        assert_eq!(m.slug, "jei");
        assert_eq!(m.class_id, Some(6));
        assert_eq!(m.latest_files_indexes[0].mod_loader, Some(ModLoaderType::Forge));
    }

    #[test]
    fn file_5754631_fixture() {
        let f = load_file();
        assert_eq!(f.file_name, "oreexcavation-1.13.174.jar");
        assert_eq!(f.file_fingerprint, 3871571640);
        assert_eq!(
            f.sha1(),
            Some("19b1540f5e69fe6d04d174915e834bb614bf51ce")
        );
        assert_eq!(f.md5(), Some("1e87b83ed930e864de2a3150255f30bf"));
        assert_eq!(f.required_mod_ids().collect::<Vec<_>>(), vec![123456]);
        assert!(f.is_approved());
    }

    #[test]
    fn unknown_loader_is_other() {
        let v: ModLoaderType = serde_json::from_str("99").unwrap();
        assert_eq!(v, ModLoaderType::Other(99));
        assert_eq!(serde_json::to_string(&v).unwrap(), "99");
    }

    #[test]
    fn pagination_next_index() {
        let p = Pagination {
            index: 0,
            page_size: 20,
            result_count: 20,
            total_count: 55,
        };
        assert_eq!(p.next_index(), Some(20));
        let end = Pagination {
            index: 40,
            page_size: 20,
            result_count: 15,
            total_count: 55,
        };
        assert_eq!(end.next_index(), None);
        let cap = Pagination {
            index: 9960,
            page_size: 50,
            result_count: 50,
            total_count: 20000,
        };
        assert_eq!(cap.next_index(), None);
    }

    #[test]
    fn file_index_matches_loader_rules() {
        let row = FileIndex {
            game_version: "1.20.1".into(),
            file_id: 1,
            filename: "a.jar".into(),
            release_type: FileReleaseType::Release,
            game_version_type_id: Some(1),
            mod_loader: Some(ModLoaderType::Forge),
        };
        assert!(row.matches("1.20.1", None));
        assert!(row.matches("1.20.1", Some(ModLoaderType::Any)));
        assert!(row.matches("1.20.1", Some(ModLoaderType::Forge)));
        assert!(!row.matches("1.20.1", Some(ModLoaderType::Fabric)));
        assert!(!row.matches("1.21.1", Some(ModLoaderType::Forge)));
        let any_row = FileIndex {
            mod_loader: Some(ModLoaderType::Any),
            ..row.clone()
        };
        assert!(any_row.matches("1.20.1", Some(ModLoaderType::Fabric)));
        let none_row = FileIndex {
            mod_loader: None,
            ..row
        };
        assert!(none_row.matches("1.20.1", Some(ModLoaderType::Forge)));
    }

    #[test]
    fn file_index_for_picks_first_match() {
        let m = load_mod();
        let idx = m.file_index_for("1.20.1", Some(ModLoaderType::Forge)).unwrap();
        assert_eq!(idx.file_id, 5700000);
        assert!(m.file_index_for("1.20.1", Some(ModLoaderType::Fabric)).is_none());
    }

    #[test]
    fn class_id_constants() {
        assert_eq!(ClassId::MODS.0, 6);
        assert_eq!(ClassId::MODPACKS.0, 4471);
        assert_eq!(ClassId::RESOURCE_PACKS.0, 12);
        assert_eq!(ClassId::SHADERS.0, 6552);
        assert_eq!(MINECRAFT_GAME_ID, 432);
        assert_eq!(DEFAULT_PAGE_SIZE, 20);
        assert_eq!(MAX_PAGE_SIZE, 50);
        assert_eq!(BATCH_SIZE, 100);
    }
}
```

`FileIndex` must be `Clone` for the `..row.clone()` test.

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib types::`

Expected: compile error (`mod types` not declared, or types missing).

- [ ] **Step 5: Implement error + types**

`crates/curseforge/src/error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http {status} for {url}")]
    Http { url: String, status: u16 },
    #[error("{kind:?} {id} not found")]
    NotFound { kind: ResourceKind, id: u32 },
    #[error("no download url for mod {mod_id} file {file_id}")]
    NoDownloadUrl { mod_id: u32, file_id: u32 },
    #[error("no compatible file for mod {mod_id} on {game_version}")]
    NoCompatibleFile { mod_id: u32, game_version: String },
    #[error("checksum mismatch for file {file_id}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        file_id: u32,
        expected: String,
        actual: String,
    },
    #[error("decode {url}: {message}")]
    Decode { url: String, message: String },
    #[error("manifest: {message}")]
    Manifest { message: String },
    #[error("zip: {message}")]
    Zip { message: String },
    #[error("invalid query: {message}")]
    InvalidQuery { message: &'static str },
    #[error("client builder: {message}")]
    Builder { message: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Mod,
    File,
}
```

`crates/curseforge/src/types.rs` — full implementation. Rules:

- Every struct: `#[derive(Debug, Clone, PartialEq)]` plus `serde::Deserialize` (and `Serialize` on enums).
- `#[serde(rename_all = "camelCase")]`, `#[serde(default)]` on the struct, unknown fields ignored (do **not** use `deny_unknown_fields`).
- Timestamps are `String`.
- `FileStatus` is `u32`. Approved is `4`.
- `ModLoaderIndexEntry.loader_type` is `#[serde(rename = "type")]`.
- `ClassId` is `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub struct ClassId(pub u32);` with:

```rust
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

Constants at the top of `types.rs`:

```rust
pub const MINECRAFT_GAME_ID: u32 = 432;
pub const DEFAULT_BASE_URL: &str = "https://api.curseforge.com";
pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 50;
pub const MAX_INDEX_PLUS_PAGE: u32 = 10_000;
pub const BATCH_SIZE: usize = 100;
```

Enum serde — one pattern, applied to `ModLoaderType`, `SortField`, `FileReleaseType`, `FileRelationType`, `HashAlgo`. Put this helper in `types.rs`:

```rust
macro_rules! cf_int_enum {
    ($name:ident, $($variant:ident => $n:expr),+ $(,)?) => {
        impl $name {
            pub fn from_u8(v: u8) -> Self {
                match v {
                    $($n => Self::$variant,)+
                    other => Self::Other(other),
                }
            }
            pub fn as_u8(self) -> u8 {
                match self {
                    $(Self::$variant => $n,)+
                    Self::Other(v) => v,
                }
            }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_u8(self.as_u8())
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Ok(Self::from_u8(u8::deserialize(d)?))
            }
        }
    };
}
```

Then:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModLoaderType {
    Any, Forge, Cauldron, LiteLoader, Fabric, Quilt, NeoForge, Other(u8),
}
cf_int_enum!(ModLoaderType, Any => 0, Forge => 1, Cauldron => 2, LiteLoader => 3, Fabric => 4, Quilt => 5, NeoForge => 6);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortField {
    Featured, Popularity, LastUpdated, Name, Author, TotalDownloads,
    Category, GameVersion, EarlyAccess, FeaturedReleased, ReleasedDate, Rating, Other(u8),
}
cf_int_enum!(SortField,
    Featured => 1, Popularity => 2, LastUpdated => 3, Name => 4, Author => 5,
    TotalDownloads => 6, Category => 7, GameVersion => 8, EarlyAccess => 9,
    FeaturedReleased => 10, ReleasedDate => 11, Rating => 12
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileReleaseType { Release, Beta, Alpha, Other(u8) }
cf_int_enum!(FileReleaseType, Release => 1, Beta => 2, Alpha => 3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileRelationType {
    EmbeddedLibrary, OptionalDependency, RequiredDependency, Tool, Incompatible, Include, Other(u8),
}
cf_int_enum!(FileRelationType,
    EmbeddedLibrary => 1, OptionalDependency => 2, RequiredDependency => 3,
    Tool => 4, Incompatible => 5, Include => 6
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgo { Sha1, Md5, Other(u8) }
cf_int_enum!(HashAlgo, Sha1 => 1, Md5 => 2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder { Asc, Desc }
```

Do **not** add 1 to `SortField` again. Wire values are the table above.

Structs — copy field lists from the spec (`Category`, `Mod`, `ModLinks`, `ModAuthor`, `ModAsset`, `FileIndex`, `File`, `FileHash`, `FileDependency`, `FileModule`, `SortableGameVersion`, `MinecraftVersion`, `ModLoaderIndexEntry`, `ModLoaderInfo`, `Page<T>`, `Pagination`). `FileIndex` needs `Clone`. `Page` and `Pagination` need `Deserialize`.

Helpers:

```rust
impl File {
    pub fn sha1(&self) -> Option<&str> {
        self.hashes.iter().find(|h| h.algo == HashAlgo::Sha1).map(|h| h.value.as_str())
    }
    pub fn md5(&self) -> Option<&str> {
        self.hashes.iter().find(|h| h.algo == HashAlgo::Md5).map(|h| h.value.as_str())
    }
    pub fn is_approved(&self) -> bool {
        self.is_available && self.file_status == 4
    }
    pub fn required_mod_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.dependencies.iter().filter_map(|d| {
            (d.relation_type == FileRelationType::RequiredDependency).then_some(d.mod_id)
        })
    }
}

impl FileIndex {
    pub fn matches(&self, mc: &str, loader: Option<ModLoaderType>) -> bool {
        if self.game_version != mc {
            return false;
        }
        match loader {
            None | Some(ModLoaderType::Any) => true,
            Some(want) => match self.mod_loader {
                None | Some(ModLoaderType::Any) => true,
                Some(have) => have == want,
            },
        }
    }
}

impl Mod {
    pub fn file_index_for(&self, mc: &str, loader: Option<ModLoaderType>) -> Option<&FileIndex> {
        self.latest_files_indexes.iter().find(|idx| idx.matches(mc, loader))
    }
}

impl Pagination {
    pub fn next_index(&self) -> Option<u32> {
        let next = self.index.checked_add(self.page_size)?;
        let next_end = next.checked_add(self.page_size)?;
        if next < self.total_count && next_end <= MAX_INDEX_PLUS_PAGE {
            Some(next)
        } else {
            None
        }
    }
}
```

Wire `0` for `alternateFileId` / `serverPackFileId` must become `Option`. Use:

```rust
fn zero_as_none<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    let v = Option::<u32>::deserialize(d)?;
    Ok(v.filter(|n| *n != 0))
}
```

on `alternate_file_id` and `server_pack_file_id`. Empty-string URLs on `ModLinks` may stay `Some("")`.

`lib.rs`:

```rust
//! CurseForge Core client for Minecraft, plus official-app key extraction.
//!
//! The catalog client never writes to disk. The key extractor reads an official
//! build and also writes nothing.

mod asar;
mod dmg;
mod extract;
mod fetch;
mod error;
mod types;

pub use extract::{CfCoreKey, CfKeyError, extract_from_bytes, extract_from_path};
pub use fetch::{extract_from_source, extract_from_url, LATEST_MAC_DMG};
pub use error::{Error, ResourceKind};
pub use types::*;
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS, including existing extract tests and the new types tests.

- [ ] **Step 7: Commit**

```bash
git add crates/curseforge/Cargo.toml crates/curseforge/src/lib.rs crates/curseforge/src/error.rs crates/curseforge/src/types.rs crates/curseforge/tests/fixtures/mod_jei.json crates/curseforge/tests/fixtures/file_5754631.json
git commit -m "$(cat <<'EOF'
feat(curseforge): add Core types, errors, and fixtures

EOF
)"
```

---

### Task 2: Fingerprint (MurmurHash2)

**Files:**
- Create: `crates/curseforge/src/fingerprint.rs`
- Modify: `crates/curseforge/src/lib.rs`
- Test: `crates/curseforge/src/fingerprint.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: nothing
- Produces: `pub fn fingerprint(bytes: &[u8]) -> u32`

- [ ] **Step 1: Write the failing test**

Create `crates/curseforge/src/fingerprint.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::fingerprint;

    #[test]
    fn golden_vectors() {
        assert_eq!(fingerprint(b""), 1540447798);
        assert_eq!(fingerprint(b" \t\r\n"), 1540447798);
        assert_eq!(fingerprint(b"a"), 626045324);
        assert_eq!(fingerprint(b"abcd"), 3376380438);
        assert_eq!(fingerprint(b"hello"), 2788266382);
        assert_eq!(fingerprint(b"he llo\n"), 2788266382);
    }
}
```

Declare `mod fingerprint;` and `pub use fingerprint::fingerprint;` in `lib.rs`. Leave `fingerprint` unimplemented so the test fails to compile or panics.

```rust
pub fn fingerprint(_bytes: &[u8]) -> u32 {
    unimplemented!("cf murmur2")
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kmine-curseforge --lib fingerprint::`

Expected: FAIL (`unimplemented` panic) or wrong value.

- [ ] **Step 3: Implement**

Replace `fingerprint` with this exact algorithm (CF: strip tab/LF/CR/space, then MurmurHash2 32-bit seed 1):

```rust
pub fn fingerprint(bytes: &[u8]) -> u32 {
    let filtered: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !matches!(b, 9 | 10 | 13 | 32))
        .collect();
    murmurhash2(&filtered, 1)
}

fn murmurhash2(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1e995;
    const R: u32 = 24;
    let mut len = data.len();
    let mut h = seed ^ (len as u32);
    let mut i = 0;
    while len >= 4 {
        let mut k = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
        i += 4;
        len -= 4;
    }
    match len {
        3 => {
            h ^= (data[i + 2] as u32) << 16;
            h ^= (data[i + 1] as u32) << 8;
            h ^= data[i] as u32;
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= (data[i + 1] as u32) << 8;
            h ^= data[i] as u32;
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= data[i] as u32;
            h = h.wrapping_mul(M);
        }
        _ => {}
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}
```

Do not add `FingerprintMatches` yet (Task 11).

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS. Golden table matches.

- [ ] **Step 5: Commit**

```bash
git add crates/curseforge/src/fingerprint.rs crates/curseforge/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): add CurseForge file fingerprint

EOF
)"
```

---

### Task 3: Manifest JSON

**Files:**
- Create: `crates/curseforge/src/manifest.rs`
- Create: `crates/curseforge/tests/fixtures/manifest_sf5.json`
- Modify: `crates/curseforge/src/lib.rs`
- Test: `crates/curseforge/src/manifest.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `Error` from Task 1
- Produces:
  - `Manifest { minecraft, manifest_type, manifest_version, name, version, author, overrides, files }`
  - `ManifestMinecraft { version, mod_loaders }`
  - `ManifestLoader { id, primary }`
  - `ManifestFile { project_id, file_id, required }`
  - `Manifest::parse(json: &[u8]) -> Result<Self, Error>`
  - `Manifest::primary_loader(&self) -> Option<&ManifestLoader>`

- [ ] **Step 1: Write the fixture**

`crates/curseforge/tests/fixtures/manifest_sf5.json`:

```json
{
  "minecraft": {
    "version": "1.20.1",
    "modLoaders": [{ "id": "forge-47.4.0", "primary": true }]
  },
  "manifestType": "minecraftModpack",
  "manifestVersion": 1,
  "name": "SkyFactory 5",
  "version": "5.0.8",
  "author": "Darkosto",
  "overrides": "overrides",
  "files": [
    { "projectID": 430225, "fileID": 5707939, "required": true }
  ]
}
```

- [ ] **Step 2: Write the failing tests**

In `manifest.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skyfactory_shaped() {
        let m = Manifest::parse(include_bytes!("../../tests/fixtures/manifest_sf5.json")).unwrap();
        assert_eq!(m.minecraft.version, "1.20.1");
        assert_eq!(m.primary_loader().unwrap().id, "forge-47.4.0");
        assert_eq!(m.files.len(), 1);
        assert_eq!(m.files[0].project_id, 430225);
        assert_eq!(m.files[0].file_id, 5707939);
        assert!(m.files[0].required);
        assert_eq!(m.overrides, "overrides");
    }

    #[test]
    fn rejects_wrong_type() {
        let err = Manifest::parse(br#"{"minecraft":{"version":"1.20.1","modLoaders":[]},"manifestType":"other","manifestVersion":1,"name":"x","version":"1","files":[]}"#).unwrap_err();
        assert!(matches!(err, crate::Error::Manifest { .. }));
    }

    #[test]
    fn rejects_empty_mc_version() {
        let err = Manifest::parse(br#"{"minecraft":{"version":"","modLoaders":[]},"manifestType":"minecraftModpack","manifestVersion":1,"name":"x","version":"1","files":[]}"#).unwrap_err();
        assert!(matches!(err, crate::Error::Manifest { .. }));
    }

    #[test]
    fn required_defaults_true_overrides_default() {
        let m = Manifest::parse(br#"{"minecraft":{"version":"1.20.1","modLoaders":[]},"manifestType":"minecraftModpack","manifestVersion":1,"name":"x","version":"1","files":[{"projectID":1,"fileID":2}]}"#).unwrap();
        assert!(m.files[0].required);
        assert_eq!(m.overrides, "overrides");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib manifest::`

Expected: compile fail (`Manifest` missing) or FAIL.

- [ ] **Step 4: Implement**

```rust
use crate::Error;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub minecraft: ManifestMinecraft,
    pub manifest_type: String,
    pub manifest_version: u32,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
    #[serde(default)]
    pub files: Vec<ManifestFile>,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestMinecraft {
    pub version: String,
    #[serde(default)]
    pub mod_loaders: Vec<ManifestLoader>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ManifestLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ManifestFile {
    #[serde(rename = "projectID")]
    pub project_id: u32,
    #[serde(rename = "fileID")]
    pub file_id: u32,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

impl Manifest {
    pub fn parse(json: &[u8]) -> Result<Self, Error> {
        let parsed: Manifest = serde_json::from_slice(json).map_err(|err| Error::Manifest {
            message: err.to_string(),
        })?;
        if parsed.manifest_type != "minecraftModpack" {
            return Err(Error::Manifest {
                message: format!("unsupported manifestType {}", parsed.manifest_type),
            });
        }
        if parsed.minecraft.version.is_empty() {
            return Err(Error::Manifest {
                message: "minecraft.version is empty".into(),
            });
        }
        Ok(parsed)
    }

    pub fn primary_loader(&self) -> Option<&ManifestLoader> {
        self.minecraft
            .mod_loaders
            .iter()
            .find(|l| l.primary)
            .or_else(|| self.minecraft.mod_loaders.first())
    }
}
```

`lib.rs`: `mod manifest;` and `pub use manifest::{Manifest, ManifestFile, ManifestLoader, ManifestMinecraft};`

Do not add `PackZip` yet (Task 10).

- [ ] **Step 5: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/curseforge/src/manifest.rs crates/curseforge/src/lib.rs crates/curseforge/tests/fixtures/manifest_sf5.json
git commit -m "$(cat <<'EOF'
feat(curseforge): parse CurseForge pack manifest.json

EOF
)"
```

---

### Task 4: Client transport, builder, categories

**Files:**
- Create: `crates/curseforge/src/client.rs`
- Create: `crates/curseforge/src/search.rs` (only `CategoryFilter` in this task)
- Modify: `crates/curseforge/src/lib.rs`
- Test: `crates/curseforge/src/client.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `Error`, `Category`, `MINECRAFT_GAME_ID`, `DEFAULT_BASE_URL` from Task 1
- Produces:
  - `pub struct Client`
  - `pub struct ClientBuilder`
  - `Client::new(api_key: impl Into<String>) -> Result<Self, Error>`
  - `Client::builder() -> ClientBuilder`
  - `ClientBuilder::{api_key, base_url, user_agent, connect_timeout, request_timeout, download_timeout, build}`
  - `Client::categories(&self, filter: CategoryFilter) -> Result<Vec<Category>, Error>`
  - `pub enum CategoryFilter { All, ClassesOnly, ChildrenOf(ClassId) }`
  - internal helpers used by later tasks: `get_data`, `get_page`, `post_data`, retry policy

- [ ] **Step 1: Write the failing tests**

Put these in `client.rs` `#[cfg(test)]`. They will not compile until `Client` exists.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CategoryFilter, ClassId, Error};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) async fn test_client(server: &MockServer) -> Client {
        Client::builder()
            .api_key("test-key")
            .base_url(server.uri())
            .build()
            .unwrap()
    }

    #[test]
    fn empty_key_is_invalid_query() {
        let err = Client::new("").unwrap_err();
        assert!(matches!(err, Error::InvalidQuery { message: "empty api key" }));
    }

    #[test]
    fn debug_hides_key() {
        let c = Client::new("super-secret-key-value").unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains("super-secret-key-value"), "{shown}");
    }

    #[tokio::test]
    async fn categories_unwraps_data() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .and(query_param("gameId", "432"))
            .and(header("x-api-key", "test-key"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":[{"id":6,"gameId":432,"name":"Mods","slug":"mc-mods","isClass":true}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let cats = test_client(&server).await.categories(CategoryFilter::All).await.unwrap();
        assert_eq!(cats.len(), 1);
        assert_eq!(cats[0].id, 6);
        assert_eq!(cats[0].name, "Mods");
    }

    #[tokio::test]
    async fn categories_classes_only_and_children() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .and(query_param("classesOnly", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .and(query_param("classId", "6"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json"))
            .expect(1)
            .mount(&server)
            .await;
        let c = test_client(&server).await;
        c.categories(CategoryFilter::ClassesOnly).await.unwrap();
        c.categories(CategoryFilter::ChildrenOf(ClassId::MODS)).await.unwrap();
    }

    #[tokio::test]
    async fn retries_503_then_ok() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json"))
            .mount(&server)
            .await;
        test_client(&server).await.categories(CategoryFilter::All).await.unwrap();
    }

    #[tokio::test]
    async fn no_retry_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let err = test_client(&server).await.categories(CategoryFilter::All).await.unwrap_err();
        assert!(matches!(err, Error::Http { status: 404, .. }));
    }
}
```

`test_client` is `pub(crate)` so later task tests in other modules can reuse it **or** each module copies this 6-line helper. Copy the helper into each test module that needs it (do not make a shared `tests/common.rs` unless you already have one). Later tasks in this plan repeat the helper so they stand alone.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib client::`

Expected: compile fail (`Client` missing).

- [ ] **Step 3: Implement search.rs (`CategoryFilter` only) and client.rs**

`search.rs` for this task:

```rust
use crate::ClassId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryFilter {
    All,
    ClassesOnly,
    ChildrenOf(ClassId),
}
```

`client.rs` essentials:

```rust
use crate::search::CategoryFilter;
use crate::types::{Category, DEFAULT_BASE_URL, MINECRAFT_GAME_ID, Page, Pagination};
use crate::Error;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const RETRIES: u32 = 2;
const RETRY_DELAY: Duration = Duration::from_millis(1000);

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    request_timeout: Duration,
    download_timeout: Duration,
}

pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: String,
    user_agent: String,
    connect_timeout: Duration,
    request_timeout: Duration,
    download_timeout: Duration,
}

impl Client {
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        Self::builder().api_key(api_key).build()
    }

    pub fn builder() -> ClientBuilder {
        ClientBuilder {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: format!("kmine-curseforge/{}", env!("CARGO_PKG_VERSION")),
            connect_timeout: CONNECT_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            download_timeout: DOWNLOAD_TIMEOUT,
        }
    }

    pub async fn categories(&self, filter: CategoryFilter) -> Result<Vec<Category>, Error> {
        let mut q = vec![("gameId".into(), MINECRAFT_GAME_ID.to_string())];
        match filter {
            CategoryFilter::All => {}
            CategoryFilter::ClassesOnly => q.push(("classesOnly".into(), "true".into())),
            CategoryFilter::ChildrenOf(class) => q.push(("classId".into(), class.0.to_string())),
        }
        self.get_data("/v1/categories", &q).await
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl ClientBuilder {
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }
    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = d;
        self
    }
    pub fn request_timeout(mut self, d: Duration) -> Self {
        self.request_timeout = d;
        self
    }
    pub fn download_timeout(mut self, d: Duration) -> Self {
        self.download_timeout = d;
        self
    }
    pub fn build(self) -> Result<Client, Error> {
        let api_key = self.api_key.unwrap_or_default();
        if api_key.is_empty() {
            return Err(Error::InvalidQuery {
                message: "empty api key",
            });
        }
        let http = reqwest::Client::builder()
            .user_agent(self.user_agent)
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .build()
            .map_err(|_| Error::Builder {
                message: "failed to build reqwest client",
            })?;
        Ok(Client {
            http,
            api_key,
            base_url: self.base_url.trim_end_matches('/').to_string(),
            request_timeout: self.request_timeout,
            download_timeout: self.download_timeout,
        })
    }
}

#[derive(serde::Deserialize)]
struct Envelope<T> {
    data: T,
    #[serde(default)]
    pagination: Option<Pagination>,
}

impl Client {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    pub(crate) async fn get_data<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<T, Error> {
        let env: Envelope<T> = self.send_retry("GET", path, query, None::<&()>).await?;
        Ok(env.data)
    }

    pub(crate) async fn get_page<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<Page<T>, Error> {
        let env: Envelope<Vec<T>> = self.send_retry("GET", path, query, None::<&()>).await?;
        Ok(Page {
            data: env.data,
            pagination: env.pagination.unwrap_or(Pagination {
                index: 0,
                page_size: 0,
                result_count: 0,
                total_count: 0,
            }),
        })
    }

    pub(crate) async fn post_data<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        let env: Envelope<T> = self.send_retry("POST", path, &[], Some(body)).await?;
        Ok(env.data)
    }

    pub(crate) async fn send_retry<T: DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        path: &str,
        query: &[(String, String)],
        body: Option<&B>,
    ) -> Result<T, Error> {
        let url = self.url(path);
        let mut last = Error::Http {
            url: url.clone(),
            status: 0,
        };
        for attempt in 0..=RETRIES {
            match self.send_once::<T, B>(method, &url, query, body).await {
                Ok(v) => return Ok(v),
                Err(err) if attempt < RETRIES && retryable(&err) => {
                    if !matches!(err, Error::Http { status: 429, .. }) {
                        tokio::time::sleep(RETRY_DELAY).await;
                    }
                    last = err;
                }
                Err(err) => return Err(err),
            }
        }
        Err(last)
    }

    async fn send_once<T: DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        url: &str,
        query: &[(String, String)],
        body: Option<&B>,
    ) -> Result<T, Error> {
        let mut req = match method {
            "POST" => self.http.post(url),
            _ => self.http.get(url),
        };
        req = req
            .header("x-api-key", &self.api_key)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(self.request_timeout)
            .query(query);
        if let Some(body) = body {
            req = req.json(body);
        }
        let response = req.send().await.map_err(|err| Error::Decode {
            url: url.to_string(),
            message: err.to_string(),
        })?;
        let status = response.status();
        if status.as_u16() == 429 {
            let delay = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(RETRY_DELAY);
            tokio::time::sleep(delay).await;
            return Err(Error::Http {
                url: url.to_string(),
                status: 429,
            });
        }
        if !status.is_success() {
            return Err(Error::Http {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        response.json::<T>().await.map_err(|err| Error::Decode {
            url: url.to_string(),
            message: err.to_string(),
        })
    }
}

fn retryable(err: &Error) -> bool {
    match err {
        Error::Http { status, .. } => *status <= 199 || *status == 429 || *status >= 500,
        Error::Decode { .. } => true,
        _ => false,
    }
}
```

429 already slept in `send_once` before returning `Error::Http`. `send_retry` still uses `RETRY_DELAY` for 5xx / decode. Do not add `Retry-After` onto the public `Error` enum.

`lib.rs` add:

```rust
mod client;
mod search;
pub use client::{Client, ClientBuilder};
pub use search::CategoryFilter;
```

`download_timeout` is stored on `Client` now even though unused until Task 9.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS. Retry test waits ~1s.

- [ ] **Step 5: Commit**

```bash
git add crates/curseforge/src/client.rs crates/curseforge/src/search.rs crates/curseforge/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): add Client transport, builder, and categories

EOF
)"
```

---

### Task 5: SearchQuery + search

**Files:**
- Modify: `crates/curseforge/src/search.rs`
- Modify: `crates/curseforge/src/client.rs`
- Modify: `crates/curseforge/src/lib.rs`
- Test: `crates/curseforge/src/search.rs` and `crates/curseforge/src/client.rs`

**Interfaces:**
- Consumes: `Client::get_page`, `Error`, `Mod`, `Page`, `ClassId`, `ModLoaderType`, `SortField`, `SortOrder`, `DEFAULT_PAGE_SIZE`, `MAX_PAGE_SIZE`, `MAX_INDEX_PLUS_PAGE`
- Produces:
  - `SearchQuery` builder methods listed below
  - `impl Default for SearchQuery` → `SearchQuery::new(ClassId::MODS)`
  - `Client::search(&self, query: &SearchQuery) -> Result<Page<Mod>, Error>`
  - `FileFilter` struct + `Default` (used in Task 7; define it here so search.rs is complete)

`SearchQuery` methods: `new(class: ClassId) -> Self`, `search(self, text: impl Into<String>) -> Self`, `categories(self, ids: impl Into<Vec<u32>>) -> Self`, `category(self, id: u32) -> Self`, `game_versions(self, versions: impl Into<Vec<String>>) -> Self`, `game_version(self, v: impl Into<String>) -> Self`, `loaders(self, loaders: impl Into<Vec<ModLoaderType>>) -> Self`, `loader(self, loader: ModLoaderType) -> Self`, `sort(self, field: SortField, order: SortOrder) -> Self`, `slug(self, slug: impl Into<String>) -> Self`, `author_id(self, id: u32) -> Self`, `game_version_type_id(self, id: u32) -> Self`, `index(self, index: u32) -> Self`, `page_size(self, page_size: u32) -> Self`.

Default sort: `SortField::Popularity` + `SortOrder::Desc`. Default `page_size`: 20. Default `index`: 0.

- [ ] **Step 1: Write the failing tests**

Append to `client.rs` tests:

```rust
    #[tokio::test]
    async fn search_encodes_json_array_query_params() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/mods/search"))
            .and(query_param("gameId", "432"))
            .and(query_param("classId", "6"))
            .and(query_param("gameVersions", r#"["1.20.1"]"#))
            .and(query_param("modLoaderTypes", "[1]"))
            .and(query_param("categoryIds", "[421]"))
            .and(query_param("searchFilter", "jei"))
            .and(query_param("sortField", "2"))
            .and(query_param("sortOrder", "desc"))
            .and(query_param("index", "0"))
            .and(query_param("pageSize", "20"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":[],"pagination":{"index":0,"pageSize":20,"resultCount":0,"totalCount":0}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let page = test_client(&server)
            .await
            .search(
                &crate::SearchQuery::new(ClassId::MODS)
                    .search("jei")
                    .game_version("1.20.1")
                    .loader(crate::ModLoaderType::Forge)
                    .category(421),
            )
            .await
            .unwrap();
        assert!(page.data.is_empty());
        assert_eq!(page.pagination.page_size, 20);
    }

    #[tokio::test]
    async fn search_rejects_page_size_51() {
        let server = MockServer::start().await;
        let err = test_client(&server)
            .await
            .search(&crate::SearchQuery::new(ClassId::MODS).page_size(51))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidQuery { .. }));
    }

    #[tokio::test]
    async fn search_rejects_eleven_categories() {
        let server = MockServer::start().await;
        let err = test_client(&server)
            .await
            .search(&crate::SearchQuery::new(ClassId::MODS).categories((1..=11).collect::<Vec<_>>()))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidQuery { .. }));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib client::tests::search_encodes`

Expected: compile fail (`SearchQuery` missing).

- [ ] **Step 3: Implement**

Add to `search.rs` (keep `CategoryFilter`):

```rust
use crate::types::{
    ClassId, DEFAULT_PAGE_SIZE, ModLoaderType, SortField, SortOrder, MAX_INDEX_PLUS_PAGE,
    MAX_PAGE_SIZE,
};
use crate::Error;

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub(crate) class: ClassId,
    pub(crate) search: Option<String>,
    pub(crate) categories: Vec<u32>,
    pub(crate) game_versions: Vec<String>,
    pub(crate) loaders: Vec<ModLoaderType>,
    pub(crate) sort_field: SortField,
    pub(crate) sort_order: SortOrder,
    pub(crate) slug: Option<String>,
    pub(crate) author_id: Option<u32>,
    pub(crate) game_version_type_id: Option<u32>,
    pub(crate) index: u32,
    pub(crate) page_size: u32,
}

impl SearchQuery {
    pub fn new(class: ClassId) -> Self {
        Self {
            class,
            search: None,
            categories: Vec::new(),
            game_versions: Vec::new(),
            loaders: Vec::new(),
            sort_field: SortField::Popularity,
            sort_order: SortOrder::Desc,
            slug: None,
            author_id: None,
            game_version_type_id: None,
            index: 0,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
    pub fn search(mut self, text: impl Into<String>) -> Self {
        self.search = Some(text.into());
        self
    }
    pub fn categories(mut self, ids: impl Into<Vec<u32>>) -> Self {
        self.categories = ids.into();
        self
    }
    pub fn category(mut self, id: u32) -> Self {
        self.categories.push(id);
        self
    }
    pub fn game_versions(mut self, versions: impl Into<Vec<String>>) -> Self {
        self.game_versions = versions.into();
        self
    }
    pub fn game_version(mut self, v: impl Into<String>) -> Self {
        self.game_versions.push(v.into());
        self
    }
    pub fn loaders(mut self, loaders: impl Into<Vec<ModLoaderType>>) -> Self {
        self.loaders = loaders.into();
        self
    }
    pub fn loader(mut self, loader: ModLoaderType) -> Self {
        self.loaders.push(loader);
        self
    }
    pub fn sort(mut self, field: SortField, order: SortOrder) -> Self {
        self.sort_field = field;
        self.sort_order = order;
        self
    }
    pub fn slug(mut self, slug: impl Into<String>) -> Self {
        self.slug = Some(slug.into());
        self
    }
    pub fn author_id(mut self, id: u32) -> Self {
        self.author_id = Some(id);
        self
    }
    pub fn game_version_type_id(mut self, id: u32) -> Self {
        self.game_version_type_id = Some(id);
        self
    }
    pub fn index(mut self, index: u32) -> Self {
        self.index = index;
        self
    }
    pub fn page_size(mut self, page_size: u32) -> Self {
        self.page_size = page_size;
        self
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        if !(1..=MAX_PAGE_SIZE).contains(&self.page_size) {
            return Err(Error::InvalidQuery {
                message: "pageSize must be 1..=50",
            });
        }
        if self.index.saturating_add(self.page_size) > MAX_INDEX_PLUS_PAGE {
            return Err(Error::InvalidQuery {
                message: "index + pageSize exceeds 10000",
            });
        }
        if self.categories.len() > 10 {
            return Err(Error::InvalidQuery {
                message: "categoryIds max 10",
            });
        }
        if self.game_versions.len() > 4 {
            return Err(Error::InvalidQuery {
                message: "gameVersions max 4",
            });
        }
        if self.loaders.len() > 5 {
            return Err(Error::InvalidQuery {
                message: "modLoaderTypes max 5",
            });
        }
        Ok(())
    }
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self::new(ClassId::MODS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

`Client::search`:

```rust
pub async fn search(&self, query: &SearchQuery) -> Result<Page<Mod>, Error> {
    query.validate()?;
    let mut q = vec![
        ("gameId".into(), MINECRAFT_GAME_ID.to_string()),
        ("classId".into(), query.class.0.to_string()),
        ("sortField".into(), query.sort_field.as_u8().to_string()),
        (
            "sortOrder".into(),
            match query.sort_order {
                SortOrder::Asc => "asc".into(),
                SortOrder::Desc => "desc".into(),
            },
        ),
        ("index".into(), query.index.to_string()),
        ("pageSize".into(), query.page_size.to_string()),
    ];
    if let Some(text) = query.search.as_ref().filter(|s| !s.is_empty()) {
        q.push(("searchFilter".into(), text.clone()));
    }
    if !query.categories.is_empty() {
        q.push(("categoryIds".into(), serde_json::to_string(&query.categories).unwrap()));
    }
    if !query.game_versions.is_empty() {
        q.push(("gameVersions".into(), serde_json::to_string(&query.game_versions).unwrap()));
    }
    if !query.loaders.is_empty() {
        let ids: Vec<u8> = query.loaders.iter().map(|l| l.as_u8()).collect();
        q.push(("modLoaderTypes".into(), serde_json::to_string(&ids).unwrap()));
    }
    if let Some(slug) = query.slug.as_ref().filter(|s| !s.is_empty()) {
        q.push(("slug".into(), slug.clone()));
    }
    if let Some(id) = query.author_id {
        q.push(("primaryAuthorId".into(), id.to_string()));
    }
    if let Some(id) = query.game_version_type_id {
        q.push(("gameVersionTypeId".into(), id.to_string()));
    }
    self.get_page("/v2/mods/search", &q).await
}
```

`lib.rs`: `pub use search::{CategoryFilter, FileFilter, SearchQuery};`

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS. If `query_param("gameVersions", r#"["1.20.1"]"#)` fails, print the incoming request in a temporary mock matcher and adjust only the **expected decoded** string so it equals what reqwest sent after decode. Do not hand-build the query string.

- [ ] **Step 5: Commit**

```bash
git add crates/curseforge/src/search.rs crates/curseforge/src/client.rs crates/curseforge/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): add SearchQuery and GET /v2/mods/search

EOF
)"
```

---

### Task 6: get_mod, get_mods, description

**Files:**
- Modify: `crates/curseforge/src/client.rs`
- Test: `crates/curseforge/src/client.rs`

**Interfaces:**
- Consumes: `Client::get_data`, `Client::post_data`, `Mod`, `Error`, `ResourceKind`, `BATCH_SIZE`
- Produces:
  - `Client::get_mod(&self, mod_id: u32) -> Result<Mod, Error>`
  - `Client::get_mods(&self, mod_ids: &[u32]) -> Result<Vec<Mod>, Error>`
  - `Client::description(&self, mod_id: u32) -> Result<String, Error>`

- [ ] **Step 1: Write the failing tests**

```rust
    #[tokio::test]
    async fn get_mod_404_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/mods/238222"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = test_client(&server).await.get_mod(238222).await.unwrap_err();
        assert!(matches!(
            err,
            Error::NotFound {
                kind: crate::ResourceKind::Mod,
                id: 238222
            }
        ));
    }

    #[tokio::test]
    async fn get_mod_ok() {
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"data":{}}}"#,
            include_str!("../../tests/fixtures/mod_jei.json")
        );
        Mock::given(method("GET"))
            .and(path("/v2/mods/238222"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;
        let m = test_client(&server).await.get_mod(238222).await.unwrap();
        assert_eq!(m.slug, "jei");
    }

    #[tokio::test]
    async fn get_mods_empty_skips_http() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let out = test_client(&server).await.get_mods(&[]).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn description_unwraps_html() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/238222/description"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":"<p>html</p>"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let html = test_client(&server).await.description(238222).await.unwrap();
        assert_eq!(html, "<p>html</p>");
    }

    #[tokio::test]
    async fn description_404_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/1/description"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = test_client(&server).await.description(1).await.unwrap_err();
        assert!(matches!(err, Error::NotFound { kind: crate::ResourceKind::Mod, id: 1 }));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib client::tests::get_mod`

Expected: compile fail (`get_mod` missing).

- [ ] **Step 3: Implement**

Add a 404 mapper used by these methods:

```rust
fn map_http(err: Error, not_found: Option<(crate::ResourceKind, u32)>) -> Error {
    match (&err, not_found) {
        (Error::Http { status: 404, .. }, Some((kind, id))) => Error::NotFound { kind, id },
        _ => err,
    }
}
```

```rust
pub async fn get_mod(&self, mod_id: u32) -> Result<Mod, Error> {
    self.get_data(&format!("/v2/mods/{mod_id}"), &[])
        .await
        .map_err(|e| map_http(e, Some((crate::ResourceKind::Mod, mod_id))))
}

pub async fn get_mods(&self, mod_ids: &[u32]) -> Result<Vec<Mod>, Error> {
    if mod_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for chunk in mod_ids.chunks(crate::BATCH_SIZE) {
        #[derive(serde::Serialize)]
        struct Body<'a> {
            #[serde(rename = "modIds")]
            mod_ids: &'a [u32],
        }
        let part: Vec<Mod> = self
            .post_data("/v2/mods/get-mods-by-ids", &Body { mod_ids: chunk })
            .await?;
        out.extend(part);
    }
    Ok(out)
}

pub async fn description(&self, mod_id: u32) -> Result<String, Error> {
    self.get_data(&format!("/v1/mods/{mod_id}/description"), &[])
        .await
        .map_err(|e| map_http(e, Some((crate::ResourceKind::Mod, mod_id))))
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/curseforge/src/client.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): add get_mod, get_mods, and description

EOF
)"
```

---

### Task 7: Files and changelog

**Files:**
- Modify: `crates/curseforge/src/client.rs`
- Test: `crates/curseforge/src/client.rs`

**Interfaces:**
- Consumes: `FileFilter` (Task 5), `File`, `Page`, `BATCH_SIZE`, `get_page`, `get_data`, `post_data`, `map_http`
- Produces:
  - `Client::files(&self, mod_id: u32, filter: &FileFilter) -> Result<Page<File>, Error>`
  - `Client::get_file(&self, mod_id: u32, file_id: u32) -> Result<File, Error>`
  - `Client::get_files(&self, file_ids: &[u32]) -> Result<Vec<File>, Error>`
  - `Client::changelog(&self, mod_id: u32, file_id: u32) -> Result<String, Error>`

`files` query: `index`, `pageSize`, optional `gameVersion`, `gameVersionTypeId`, `modLoaderType` (single int via `as_u8`), `clientCompatible`. Validate `page_size` 1..=50 and `index + page_size <= 10000` with the same `InvalidQuery` messages as `SearchQuery::validate`.

`get_file` / `changelog` 404 → `Error::NotFound { kind: File, id: file_id }`.

`get_files` empty → `Ok(vec![])` no HTTP. Chunk at 100. Body `{ "fileIds": [...] }`. Order not preserved.

- [ ] **Step 1: Write the failing tests**

```rust
    #[tokio::test]
    async fn get_file_404_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/250898/files/5754631"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = test_client(&server).await.get_file(250898, 5754631).await.unwrap_err();
        assert!(matches!(
            err,
            Error::NotFound {
                kind: crate::ResourceKind::File,
                id: 5754631
            }
        ));
    }

    #[tokio::test]
    async fn files_sends_single_loader_and_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/238222/files"))
            .and(query_param("gameVersion", "1.20.1"))
            .and(query_param("modLoaderType", "1"))
            .and(query_param("index", "0"))
            .and(query_param("pageSize", "20"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":[],"pagination":{"index":0,"pageSize":20,"resultCount":0,"totalCount":0}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        test_client(&server)
            .await
            .files(
                238222,
                &crate::FileFilter {
                    game_version: Some("1.20.1".into()),
                    loader: Some(crate::ModLoaderType::Forge),
                    ..crate::FileFilter::default()
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn get_files_chunks_101_ids() {
        use std::sync::{Arc, Mutex};
        let server = MockServer::start().await;
        let sizes = Arc::new(Mutex::new(Vec::new()));
        let sizes2 = sizes.clone();
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(move |req: &wiremock::Request| {
                let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = v["fileIds"].as_array().unwrap().len();
                sizes2.lock().unwrap().push(n);
                ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json")
            })
            .expect(2)
            .mount(&server)
            .await;
        let ids: Vec<u32> = (1..=101).collect();
        test_client(&server).await.get_files(&ids).await.unwrap();
        let got = sizes.lock().unwrap().clone();
        assert_eq!(got, vec![100, 1]);
    }

    #[tokio::test]
    async fn changelog_unwraps_html() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/1/files/2/changelog"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":"<p>notes</p>"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let html = test_client(&server).await.changelog(1, 2).await.unwrap();
        assert_eq!(html, "<p>notes</p>");
    }

    #[tokio::test]
    async fn get_files_empty_skips_http() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        assert!(test_client(&server).await.get_files(&[]).await.unwrap().is_empty());
    }
```

If `respond_with` closure is not supported by this wiremock 0.6 API, use `wiremock::Respond` impl:

```rust
struct CaptureSizes(Arc<Mutex<Vec<usize>>>);
impl wiremock::Respond for CaptureSizes {
    fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
        let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
        self.0.lock().unwrap().push(v["fileIds"].as_array().unwrap().len());
        ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json")
    }
}
```

Use `CaptureSizes` if the `respond_with` closure form does not compile on wiremock 0.6.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib client::tests::get_files_chunks`

Expected: compile fail.

- [ ] **Step 3: Implement**

```rust
pub async fn files(&self, mod_id: u32, filter: &FileFilter) -> Result<Page<File>, Error> {
    if !(1..=crate::MAX_PAGE_SIZE).contains(&filter.page_size) {
        return Err(Error::InvalidQuery {
            message: "pageSize must be 1..=50",
        });
    }
    if filter.index.saturating_add(filter.page_size) > crate::MAX_INDEX_PLUS_PAGE {
        return Err(Error::InvalidQuery {
            message: "index + pageSize exceeds 10000",
        });
    }
    let mut q = vec![
        ("index".into(), filter.index.to_string()),
        ("pageSize".into(), filter.page_size.to_string()),
    ];
    if let Some(v) = &filter.game_version {
        q.push(("gameVersion".into(), v.clone()));
    }
    if let Some(id) = filter.game_version_type_id {
        q.push(("gameVersionTypeId".into(), id.to_string()));
    }
    if let Some(loader) = filter.loader {
        q.push(("modLoaderType".into(), loader.as_u8().to_string()));
    }
    if let Some(cc) = filter.client_compatible {
        q.push(("clientCompatible".into(), if cc { "true" } else { "false" }.into()));
    }
    self.get_page(&format!("/v1/mods/{mod_id}/files"), &q).await
}

pub async fn get_file(&self, mod_id: u32, file_id: u32) -> Result<File, Error> {
    self.get_data(&format!("/v1/mods/{mod_id}/files/{file_id}"), &[])
        .await
        .map_err(|e| map_http(e, Some((crate::ResourceKind::File, file_id))))
}

pub async fn get_files(&self, file_ids: &[u32]) -> Result<Vec<File>, Error> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for chunk in file_ids.chunks(crate::BATCH_SIZE) {
        #[derive(serde::Serialize)]
        struct Body<'a> {
            #[serde(rename = "fileIds")]
            file_ids: &'a [u32],
        }
        let part: Vec<File> = self
            .post_data("/v1/mods/files", &Body { file_ids: chunk })
            .await?;
        out.extend(part);
    }
    Ok(out)
}

pub async fn changelog(&self, mod_id: u32, file_id: u32) -> Result<String, Error> {
    self.get_data(&format!("/v1/mods/{mod_id}/files/{file_id}/changelog"), &[])
        .await
        .map_err(|e| map_http(e, Some((crate::ResourceKind::File, file_id))))
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/curseforge/src/client.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): add file list, batch, and changelog

EOF
)"
```

---

### Task 8: Minecraft versions and loaders

**Files:**
- Modify: `crates/curseforge/src/client.rs`
- Test: `crates/curseforge/src/client.rs`

**Interfaces:**
- Consumes: `MinecraftVersion`, `ModLoaderIndexEntry`, `ModLoaderInfo`, `get_data`
- Produces:
  - `Client::minecraft_versions(&self) -> Result<Vec<MinecraftVersion>, Error>`
  - `Client::minecraft_version(&self, version: &str) -> Result<MinecraftVersion, Error>`
  - `Client::modloaders(&self) -> Result<Vec<ModLoaderIndexEntry>, Error>`
  - `Client::modloader(&self, name: &str) -> Result<ModLoaderInfo, Error>`

404 on `minecraft_version` / `modloader` stays `Error::Http { status: 404, .. }` (string keys).

`modloaders` always sends `includeAll=true`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[tokio::test]
    async fn minecraft_versions_and_one() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/minecraft/version"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":[{"versionString":"1.20.1","approved":true}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/minecraft/version/1.20.1"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":{"versionString":"1.20.1"}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let c = test_client(&server).await;
        assert_eq!(c.minecraft_versions().await.unwrap()[0].version_string, "1.20.1");
        assert_eq!(c.minecraft_version("1.20.1").await.unwrap().version_string, "1.20.1");
    }

    #[tokio::test]
    async fn modloaders_include_all_and_one() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/minecraft/modloader"))
            .and(query_param("includeAll", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":[{"name":"forge-47.4.0","gameVersion":"1.20.1","latest":false,"recommended":true,"type":1}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/minecraft/modloader/forge-47.4.0"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":{"name":"forge-47.4.0","gameVersion":"1.20.1","latest":false,"recommended":true,"downloadUrl":"https://modloaders.forgecdn.net/x","filename":"forge-1.20.1-47.4.0.jar","installMethod":3,"versionJson":{"a":1},"installProfileJson":{"b":2}}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let c = test_client(&server).await;
        let idx = c.modloaders().await.unwrap();
        assert_eq!(idx[0].name, "forge-47.4.0");
        assert_eq!(idx[0].loader_type, Some(crate::ModLoaderType::Forge));
        let one = c.modloader("forge-47.4.0").await.unwrap();
        assert_eq!(one.install_method, Some(3));
        assert!(one.version_json.is_some());
    }

    #[tokio::test]
    async fn modloader_404_is_http() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/minecraft/modloader/nope"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = test_client(&server).await.modloader("nope").await.unwrap_err();
        assert!(matches!(err, Error::Http { status: 404, .. }));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib client::tests::modloaders`

Expected: compile fail.

- [ ] **Step 3: Implement**

```rust
pub async fn minecraft_versions(&self) -> Result<Vec<MinecraftVersion>, Error> {
    self.get_data("/v1/minecraft/version", &[]).await
}

pub async fn minecraft_version(&self, version: &str) -> Result<MinecraftVersion, Error> {
    self.get_data(&format!("/v1/minecraft/version/{version}"), &[]).await
}

pub async fn modloaders(&self) -> Result<Vec<ModLoaderIndexEntry>, Error> {
    self.get_data(
        "/v1/minecraft/modloader",
        &[("includeAll".into(), "true".into())],
    )
    .await
}

pub async fn modloader(&self, name: &str) -> Result<ModLoaderInfo, Error> {
    self.get_data(&format!("/v1/minecraft/modloader/{name}"), &[]).await
}
```

Import `MinecraftVersion`, `ModLoaderIndexEntry`, `ModLoaderInfo` in `client.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/curseforge/src/client.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): add Minecraft version and modloader endpoints

EOF
)"
```

---

### Task 9: Download, CDN, SHA-1

**Files:**
- Create: `crates/curseforge/src/download.rs`
- Modify: `crates/curseforge/src/client.rs`
- Modify: `crates/curseforge/src/lib.rs`
- Test: `crates/curseforge/src/download.rs`, `crates/curseforge/src/client.rs`

**Interfaces:**
- Consumes: `File`, `Error`, `Client` (http, `download_timeout`; **no** `x-api-key` on CDN)
- Produces:
  - `pub struct Downloaded { file_id: u32, file_name: String, bytes: Bytes, sha1: Option<String> }`
  - `pub fn cdn_file_url(file_id: u32, file_name: &str) -> String`
  - `pub fn resolve_download_url(file: &File) -> Option<String>`
  - `Client::download(&self, file: &File) -> Result<Downloaded, Error>`

`cdn_file_url(5754631, "oreexcavation-1.13.174.jar")` ==
`https://edge.forgecdn.net/files/5754/631/oreexcavation-1.13.174.jar`

`resolve_download_url`: non-empty `download_url` else `cdn_file_url` if `file_name` non-empty else `None`.

`download`:
1. Resolve URL. `None` → `Error::NoDownloadUrl { mod_id, file_id }`.
2. GET without `x-api-key`, `download_timeout`.
3. If that URL was the API `download_url` and the GET fails with HTTP, try `cdn_file_url` once, then normal 429/5xx retries on the CDN URL.
4. Collect `Bytes`.
5. SHA-1 the bytes (lowercase hex). If `file.sha1()` is `Some` and mismatches → `Error::ChecksumMismatch`. If it matches, `Downloaded.sha1 = Some(hex)`. If no advertised SHA-1, still set `Downloaded.sha1 = Some(computed)`.
6. Do not write files. Do not check MD5. Do not refuse `allow_mod_distribution == false`.

- [ ] **Step 1: Write the failing tests**

`download.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{File, FileHash, FileReleaseType, HashAlgo};

    fn stub_file(url: Option<&str>, name: &str) -> File {
        File {
            id: 5754631,
            game_id: 432,
            mod_id: 250898,
            is_available: true,
            display_name: name.into(),
            file_name: name.into(),
            release_type: FileReleaseType::Release,
            file_status: 4,
            hashes: vec![],
            file_date: None,
            file_length: 0,
            download_count: 0,
            download_url: url.map(str::to_string),
            game_versions: vec![],
            sortable_game_versions: vec![],
            dependencies: vec![],
            alternate_file_id: None,
            is_server_pack: false,
            server_pack_file_id: None,
            is_early_access_content: false,
            file_fingerprint: 0,
            modules: vec![],
        }
    }

    #[test]
    fn cdn_split() {
        assert_eq!(
            cdn_file_url(5754631, "oreexcavation-1.13.174.jar"),
            "https://edge.forgecdn.net/files/5754/631/oreexcavation-1.13.174.jar"
        );
    }

    #[test]
    fn resolve_prefers_download_url() {
        let f = stub_file(Some("https://cdn.example/a.jar"), "a.jar");
        assert_eq!(resolve_download_url(&f).unwrap(), "https://cdn.example/a.jar");
        let f = stub_file(None, "oreexcavation-1.13.174.jar");
        assert_eq!(
            resolve_download_url(&f).unwrap(),
            "https://edge.forgecdn.net/files/5754/631/oreexcavation-1.13.174.jar"
        );
        let mut f = stub_file(Some(""), "");
        f.file_name.clear();
        assert!(resolve_download_url(&f).is_none());
    }
}
```

`File` is a large struct — if Task 1 did not derive a way to build one, construct it field-by-field as above. If you added `Default` on `File` in Task 1, use `File { id: …, file_name: …, ..File::default() }` instead.

`client.rs` download tests:

```rust
    fn file_for_download(url: Option<String>, sha1: Option<&str>) -> crate::File {
        crate::File {
            id: 7,
            game_id: 432,
            mod_id: 3,
            is_available: true,
            display_name: "a.jar".into(),
            file_name: "a.jar".into(),
            release_type: crate::FileReleaseType::Release,
            file_status: 4,
            hashes: sha1
                .map(|h| vec![crate::FileHash {
                    algo: crate::HashAlgo::Sha1,
                    value: h.into(),
                }])
                .unwrap_or_default(),
            file_date: None,
            file_length: 3,
            download_count: 0,
            download_url: url,
            game_versions: vec![],
            sortable_game_versions: vec![],
            dependencies: vec![],
            alternate_file_id: None,
            is_server_pack: false,
            server_pack_file_id: None,
            is_early_access_content: false,
            file_fingerprint: 0,
            modules: vec![],
        }
    }

    #[tokio::test]
    async fn download_uses_url_without_api_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/dl/a.jar"))
            .and(header("x-api-key", "test-key"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/dl/a.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"abc".as_slice(), "application/java"))
            .mount(&server)
            .await;
        let url = format!("{}/dl/a.jar", server.uri());
        let got = test_client(&server)
            .await
            .download(&file_for_download(Some(url), None))
            .await
            .unwrap();
        assert_eq!(&got.bytes[..], b"abc");
        assert_eq!(got.file_id, 7);
        assert_eq!(
            got.sha1.as_deref(),
            Some("a9993e364706816aba3e25717850c26c9cd0d89d")
        );
    }

    #[tokio::test]
    async fn download_sha1_mismatch() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"abc".as_slice(), "application/octet-stream"))
            .mount(&server)
            .await;
        let url = format!("{}/x", server.uri());
        let err = test_client(&server)
            .await
            .download(&file_for_download(Some(url), Some("deadbeef")))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::ChecksumMismatch { file_id: 7, .. }));
    }

    #[tokio::test]
    async fn download_null_url_hits_cdn_path() {
        // CDN host is edge.forgecdn.net — cannot intercept. Test resolve + a mock
        // by setting download_url to None and file_name empty → NoDownloadUrl.
        let server = MockServer::start().await;
        let mut f = file_for_download(None, None);
        f.file_name.clear();
        let err = test_client(&server).await.download(&f).await.unwrap_err();
        assert!(matches!(err, Error::NoDownloadUrl { mod_id: 3, file_id: 7 }));
    }
```

The "no x-api-key" assertion: a mock that requires `x-api-key: test-key` and `expect(0)` plus a mock that accepts the GET without that header. If wiremock still matches the first (header matcher is AND), `expect(0)` fails if the header is sent.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib download::`

Expected: compile fail.

- [ ] **Step 3: Implement `download.rs` and `Client::download`**

```rust
use crate::types::File;
use bytes::Bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub file_id: u32,
    pub file_name: String,
    pub bytes: Bytes,
    pub sha1: Option<String>,
}

pub fn cdn_file_url(file_id: u32, file_name: &str) -> String {
    let folder = file_id / 1000;
    let leaf = file_id % 1000;
    let name = urlencoding_file_name(file_name);
    format!("https://edge.forgecdn.net/files/{folder}/{leaf}/{name}")
}

fn urlencoding_file_name(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn resolve_download_url(file: &File) -> Option<String> {
    if let Some(url) = file.download_url.as_ref().filter(|u| !u.is_empty()) {
        return Some(url.clone());
    }
    if !file.file_name.is_empty() {
        return Some(cdn_file_url(file.id, &file.file_name));
    }
    None
}
```

Do not add a `urlencoding` crate. The jar name in the golden URL has no special characters; encoding is for spaces/`+`.

`Client::download` in `client.rs`:

```rust
pub async fn download(&self, file: &File) -> Result<crate::Downloaded, Error> {
    let primary = crate::resolve_download_url(file).ok_or(Error::NoDownloadUrl {
        mod_id: file.mod_id,
        file_id: file.id,
    })?;
    let used_api_url = file
        .download_url
        .as_ref()
        .is_some_and(|u| !u.is_empty() && u == &primary);
    let bytes = match self.download_bytes(&primary).await {
        Ok(b) => b,
        Err(err) if used_api_url && !file.file_name.is_empty() => {
            let cdn = crate::cdn_file_url(file.id, &file.file_name);
            self.download_bytes(&cdn).await?
        }
        Err(err) => return Err(err),
    };
    let actual = {
        use sha1::{Digest, Sha1};
        hex::encode(Sha1::digest(&bytes))
    };
    if let Some(expected) = file.sha1() {
        if actual != expected.to_ascii_lowercase() {
            return Err(Error::ChecksumMismatch {
                file_id: file.id,
                expected: expected.to_ascii_lowercase(),
                actual,
            });
        }
    }
    Ok(crate::Downloaded {
        file_id: file.id,
        file_name: file.file_name.clone(),
        bytes,
        sha1: Some(actual),
    })
}

async fn download_bytes(&self, url: &str) -> Result<bytes::Bytes, Error> {
    let mut last = Error::Http {
        url: url.to_string(),
        status: 0,
    };
    for attempt in 0..=RETRIES {
        match self.download_bytes_once(url).await {
            Ok(b) => return Ok(b),
            Err(err) if attempt < RETRIES && retryable(&err) => {
                tokio::time::sleep(RETRY_DELAY).await;
                last = err;
            }
            Err(err) => return Err(err),
        }
    }
    Err(last)
}

async fn download_bytes_once(&self, url: &str) -> Result<bytes::Bytes, Error> {
    let response = self
        .http
        .get(url)
        .timeout(self.download_timeout)
        .send()
        .await
        .map_err(|err| Error::Decode {
            url: url.to_string(),
            message: err.to_string(),
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(Error::Http {
            url: url.to_string(),
            status: status.as_u16(),
        });
    }
    response.bytes().await.map_err(|err| Error::Decode {
        url: url.to_string(),
        message: err.to_string(),
    })
}
```

`lib.rs`: `mod download;` `pub use download::{cdn_file_url, resolve_download_url, Downloaded};`

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS. SHA-1 of `abc` is `a9993e364706816aba3e25717850c26c9cd0d89d`.

- [ ] **Step 5: Commit**

```bash
git add crates/curseforge/src/download.rs crates/curseforge/src/client.rs crates/curseforge/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): download file bytes with CDN fallback and SHA-1

EOF
)"
```

---

### Task 10: PackZip in memory

**Files:**
- Modify: `crates/curseforge/src/manifest.rs`
- Modify: `crates/curseforge/src/lib.rs`
- Test: `crates/curseforge/src/manifest.rs`

**Interfaces:**
- Consumes: `Manifest::parse`, `Error`, `bytes::Bytes`, `zip`
- Produces:
  - `pub struct PackZip`
  - `pub struct PackOverride { relative_path: String, bytes: Bytes }`
  - `PackZip::parse(bytes: impl Into<Bytes>) -> Result<Self, Error>`
  - `PackZip::manifest(&mut self) -> Result<Manifest, Error>`
  - `PackZip::next_override(&mut self) -> Option<Result<PackOverride, Error>>`

Rules:
- Open zip from memory. No filesystem.
- `manifest()` reads `manifest.json` at zip root. If there is no root `manifest.json` and the zip has exactly one top-level directory containing `manifest.json`, use that one-level prefix. Otherwise `Error::Manifest`.
- `next_override` yields files under `{prefix}{overrides}/` (default overrides folder `"overrides"`), skipping directories. Paths use `/` and are relative to the overrides folder (`config/a.txt`, not `overrides/config/a.txt`). Lazy — do not preload.

- [ ] **Step 1: Write the failing tests**

Build tiny zips in the test with the `zip` crate (`ZipWriter` + `Cursor<Vec<u8>>`).

```rust
    fn zip_with(entries: &[(&str, &[u8])]) -> bytes::Bytes {
        use std::io::{Cursor, Write};
        let mut buf = Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        bytes::Bytes::from(buf.into_inner())
    }

    #[test]
    fn pack_zip_root_manifest_and_override() {
        let json = include_bytes!("../../tests/fixtures/manifest_sf5.json");
        let bytes = zip_with(&[
            ("manifest.json", json.as_slice()),
            ("overrides/config/a.txt", b"hi"),
            ("overrides/", b""),
        ]);
        let mut pack = PackZip::parse(bytes).unwrap();
        let m = pack.manifest().unwrap();
        assert_eq!(m.name, "SkyFactory 5");
        let over = pack.next_override().unwrap().unwrap();
        assert_eq!(over.relative_path, "config/a.txt");
        assert_eq!(&over.bytes[..], b"hi");
        assert!(pack.next_override().is_none());
    }

    #[test]
    fn pack_zip_wrapped_folder() {
        let json = include_bytes!("../../tests/fixtures/manifest_sf5.json");
        let bytes = zip_with(&[
            ("SF5/manifest.json", json.as_slice()),
            ("SF5/overrides/config/a.txt", b"hi"),
        ]);
        let mut pack = PackZip::parse(bytes).unwrap();
        assert_eq!(pack.manifest().unwrap().name, "SkyFactory 5");
        let over = pack.next_override().unwrap().unwrap();
        assert_eq!(over.relative_path, "config/a.txt");
    }

    #[test]
    fn pack_zip_two_roots_without_manifest_errors() {
        let bytes = zip_with(&[("a/foo.txt", b"1"), ("b/bar.txt", b"2")]);
        let mut pack = PackZip::parse(bytes).unwrap();
        assert!(matches!(pack.manifest().unwrap_err(), crate::Error::Manifest { .. }));
    }
```

If `ZipWriter` rejects directory entries with empty data, omit `("overrides/", b"")`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib manifest::tests::pack_zip`

Expected: compile fail (`PackZip` missing).

- [ ] **Step 3: Implement**

```rust
use bytes::Bytes;
use std::io::{Cursor, Read};
use zip::ZipArchive;

pub struct PackOverride {
    pub relative_path: String,
    pub bytes: Bytes,
}

pub struct PackZip {
    archive: ZipArchive<Cursor<Bytes>>,
    prefix: Option<String>,
    overrides: String,
    next_index: usize,
    ready: bool,
}

impl PackZip {
    pub fn parse(bytes: impl Into<Bytes>) -> Result<Self, Error> {
        let bytes = bytes.into();
        let archive = ZipArchive::new(Cursor::new(bytes)).map_err(|err| Error::Zip {
            message: err.to_string(),
        })?;
        Ok(Self {
            archive,
            prefix: None,
            overrides: "overrides".into(),
            next_index: 0,
            ready: false,
        })
    }

    pub fn manifest(&mut self) -> Result<Manifest, Error> {
        let (name, prefix) = find_manifest_name(&self.archive)?;
        let mut file = self.archive.by_name(&name).map_err(|err| Error::Zip {
            message: err.to_string(),
        })?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|err| Error::Zip {
            message: err.to_string(),
        })?;
        drop(file);
        let parsed = Manifest::parse(&buf)?;
        self.prefix = prefix;
        self.overrides = parsed.overrides.clone();
        self.ready = true;
        self.next_index = 0;
        Ok(parsed)
    }

    pub fn next_override(&mut self) -> Option<Result<PackOverride, Error>> {
        if !self.ready {
            return Some(Err(Error::Manifest {
                message: "manifest() not called".into(),
            }));
        }
        let prefix = self.prefix.clone().unwrap_or_default();
        let root = if prefix.is_empty() {
            format!("{}/", self.overrides.trim_end_matches('/'))
        } else {
            format!(
                "{}/{}/",
                prefix.trim_end_matches('/'),
                self.overrides.trim_end_matches('/')
            )
        };
        while self.next_index < self.archive.len() {
            let i = self.next_index;
            self.next_index += 1;
            let mut file = match self.archive.by_index(i) {
                Ok(f) => f,
                Err(err) => return Some(Err(Error::Zip { message: err.to_string() })),
            };
            if file.is_dir() {
                continue;
            }
            let name = file.name().replace('\\', "/");
            let Some(rel) = name.strip_prefix(&root) else {
                continue;
            };
            if rel.is_empty() {
                continue;
            }
            let mut buf = Vec::new();
            if let Err(err) = file.read_to_end(&mut buf) {
                return Some(Err(Error::Zip {
                    message: err.to_string(),
                }));
            }
            return Some(Ok(PackOverride {
                relative_path: rel.to_string(),
                bytes: Bytes::from(buf),
            }));
        }
        None
    }
}

fn find_manifest_name<R: std::io::Read + std::io::Seek>(
    archive: &ZipArchive<R>,
) -> Result<(String, Option<String>), Error> {
    let names: Vec<String> = archive.file_names().map(|s| s.replace('\\', "/")).collect();
    if names.iter().any(|n| n == "manifest.json") {
        return Ok(("manifest.json".into(), None));
    }
    let mut tops = std::collections::BTreeSet::new();
    for n in &names {
        let top = n.split('/').next().unwrap_or(n);
        if !top.is_empty() {
            tops.insert(top.to_string());
        }
    }
    if tops.len() == 1 {
        let top = tops.iter().next().unwrap();
        let candidate = format!("{top}/manifest.json");
        if names.iter().any(|n| n == &candidate) {
            return Ok((candidate, Some(top.clone())));
        }
    }
    Err(Error::Manifest {
        message: "manifest.json not found".into(),
    })
}
```

In `find_manifest_name` keep **only** the `file_names()` version. Delete the first `names` collection that uses `nth`.

`lib.rs`: `pub use manifest::{Manifest, ManifestFile, ManifestLoader, ManifestMinecraft, PackOverride, PackZip};`

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/curseforge/src/manifest.rs crates/curseforge/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): walk pack zip manifests and overrides in memory

EOF
)"
```

---

### Task 11: resolve_pack, resolve_required_deps, fingerprints HTTP

**Files:**
- Modify: `crates/curseforge/src/manifest.rs` (`ResolvedPack`, `ResolvedPackFile`)
- Modify: `crates/curseforge/src/fingerprint.rs` (`FingerprintMatches`, `FingerprintMatch`)
- Modify: `crates/curseforge/src/client.rs`
- Modify: `crates/curseforge/src/lib.rs`
- Test: `crates/curseforge/src/client.rs`

**Interfaces:**
- Consumes: `Client::get_files`, `Client::get_mod`, `Client::get_file`, `Manifest`, `File`, `Mod::file_index_for`, `File::required_mod_ids`, `Error`, `ResourceKind`, `BATCH_SIZE`
- Produces:
  - `pub struct ResolvedPack { manifest: Manifest, files: Vec<ResolvedPackFile> }`
  - `pub struct ResolvedPackFile { project_id: u32, file_id: u32, required: bool, file: File }`
  - `Client::resolve_pack(&self, manifest: &Manifest) -> Result<ResolvedPack, Error>`
  - `Client::resolve_required_deps(&self, roots: &[File], game_version: &str, loader: Option<ModLoaderType>) -> Result<Vec<File>, Error>`
  - `pub struct FingerprintMatches { exact: Vec<FingerprintMatch>, unmatched: Vec<u32> }`
  - `pub struct FingerprintMatch { id: u32, file: File, latest_files: Vec<File> }`
  - `Client::fingerprints(&self, fingerprints: &[u32]) -> Result<FingerprintMatches, Error>`

`resolve_pack`:
1. Collect all `file_id`s, `get_files` in chunks of 100.
2. Required row with no `File` → `Error::NotFound { kind: File, id }`.
3. Optional missing row → omit.
4. Do not download jars. Every required row is in `files`.

`resolve_required_deps`:
1. `seen` is a `HashSet<u32>` of **mod ids**. Insert every `roots[i].mod_id`.
2. Queue required dep mod ids from roots not in `seen`.
3. Pop `mod_id`, insert into `seen`, `get_mod` → `file_index_for(game_version, loader)` → `get_file`. No index → `Error::NoCompatibleFile { mod_id, game_version }`.
4. Push that `File`, enqueue its required dep mod ids not in `seen`.
5. Return only new files (not roots). Cap 512 fetched files → `Error::InvalidQuery { message: "dependency walk exceeded 512" }`.

`fingerprints`:
- Empty input → `Ok` empty, no HTTP.
- `POST /v1/fingerprints/432` body `{ "fingerprints": [...] }`.
- Map `data.exactMatches` → `exact` (`id`, `file`, `latestFiles`).
- Map `data.unmatchedFingerprints` → `unmatched` (default empty).

- [ ] **Step 1: Write the failing tests**

Need a `File` JSON helper. Reuse `file_5754631.json` and tweak ids in-line.

```rust
    fn file_json(id: u32, mod_id: u32, req: Option<u32>) -> String {
        let dep = match req {
            Some(mid) => format!(r#"[{{"modId":{mid},"relationType":3}}]"#),
            None => "[]".into(),
        };
        format!(
            r#"{{"id":{id},"gameId":432,"modId":{mod_id},"isAvailable":true,"displayName":"f.jar","fileName":"f.jar","releaseType":1,"fileStatus":4,"hashes":[],"fileLength":1,"downloadCount":0,"gameVersions":[],"sortableGameVersions":[],"dependencies":{dep},"isServerPack":false,"isEarlyAccessContent":false,"fileFingerprint":1,"modules":[]}}"#
        )
    }

    #[tokio::test]
    async fn resolve_pack_batches_and_drops_optional_missing() {
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"data":[{}]}}"#,
            file_json(5707939, 430225, None)
        );
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;
        let manifest = crate::Manifest::parse(include_bytes!("../../tests/fixtures/manifest_sf5.json")).unwrap();
        // add an optional missing file in a cloned-like parse by reconstructing
        let mut manifest = manifest;
        manifest.files.push(crate::ManifestFile {
            project_id: 9,
            file_id: 9,
            required: false,
        });
        let resolved = test_client(&server).await.resolve_pack(&manifest).await.unwrap();
        assert_eq!(resolved.files.len(), 1);
        assert_eq!(resolved.files[0].file_id, 5707939);
        assert_eq!(resolved.files[0].file.mod_id, 430225);
    }

    #[tokio::test]
    async fn resolve_pack_missing_required_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json"))
            .mount(&server)
            .await;
        let manifest = crate::Manifest::parse(include_bytes!("../../tests/fixtures/manifest_sf5.json")).unwrap();
        let err = test_client(&server).await.resolve_pack(&manifest).await.unwrap_err();
        assert!(matches!(
            err,
            Error::NotFound {
                kind: crate::ResourceKind::File,
                id: 5707939
            }
        ));
    }

    #[tokio::test]
    async fn resolve_required_deps_returns_only_new_file() {
        let server = MockServer::start().await;
        let mod_body = r#"{"data":{"id":2,"gameId":432,"name":"lib","slug":"lib","links":{},"summary":"","status":4,"downloadCount":0,"isFeatured":false,"categories":[],"authors":[],"screenshots":[],"latestFiles":[],"latestFilesIndexes":[{"gameVersion":"1.20.1","fileId":22,"filename":"lib.jar","releaseType":1,"modLoader":1}],"isAvailable":true,"thumbsUpCount":0}}"#;
        Mock::given(method("GET"))
            .and(path("/v2/mods/2"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(mod_body.as_bytes(), "application/json"))
            .mount(&server)
            .await;
        let file_body = format!(r#"{{"data":{}}}"#, file_json(22, 2, None));
        Mock::given(method("GET"))
            .and(path("/v1/mods/2/files/22"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(file_body, "application/json"))
            .mount(&server)
            .await;
        let root: crate::File = serde_json::from_str(&file_json(11, 1, Some(2))).unwrap();
        let deps = test_client(&server)
            .await
            .resolve_required_deps(&[root], "1.20.1", Some(crate::ModLoaderType::Forge))
            .await
            .unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].id, 22);
        assert_eq!(deps[0].mod_id, 2);
    }

    #[tokio::test]
    async fn fingerprints_maps_exact_and_unmatched() {
        let server = MockServer::start().await;
        let file = file_json(5754631, 250898, None);
        let body = format!(
            r#"{{"data":{{"exactMatches":[{{"id":5754631,"file":{file},"latestFiles":[]}}],"unmatchedFingerprints":[9]}}}}"#
        );
        Mock::given(method("POST"))
            .and(path("/v1/fingerprints/432"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;
        let got = test_client(&server)
            .await
            .fingerprints(&[3871571640, 9])
            .await
            .unwrap();
        assert_eq!(got.exact.len(), 1);
        assert_eq!(got.exact[0].id, 5754631);
        assert_eq!(got.unmatched, vec![9]);
    }

    #[tokio::test]
    async fn fingerprints_empty_skips_http() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let got = test_client(&server).await.fingerprints(&[]).await.unwrap();
        assert!(got.exact.is_empty());
        assert!(got.unmatched.is_empty());
    }
```

`Manifest.files` must be public (Task 3). `ManifestFile` fields public.

If `ModLinks` does not default from `{}`, add `#[serde(default)]` on `Mod.links` in Task 1 (do it now if the deps test fails to deserialize the slim mod JSON).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kmine-curseforge --lib client::tests::resolve_pack`

Expected: compile fail.

- [ ] **Step 3: Implement**

In `manifest.rs`:

```rust
use crate::types::File;

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPack {
    pub manifest: Manifest,
    pub files: Vec<ResolvedPackFile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPackFile {
    pub project_id: u32,
    pub file_id: u32,
    pub required: bool,
    pub file: File,
}
```

In `fingerprint.rs`:

```rust
use crate::types::File;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct FingerprintMatches {
    pub exact: Vec<FingerprintMatch>,
    pub unmatched: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FingerprintMatch {
    pub id: u32,
    pub file: File,
    pub latest_files: Vec<File>,
}

#[derive(Deserialize)]
pub(crate) struct FingerprintEnvelope {
    #[serde(default)]
    pub exact_matches: Vec<ExactMatch>,
    #[serde(default)]
    pub unmatched_fingerprints: Vec<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExactMatch {
    pub id: u32,
    pub file: File,
    #[serde(default)]
    pub latest_files: Vec<File>,
}
```

`FingerprintEnvelope` needs `rename_all = "camelCase"` as well.

`Client` methods:

```rust
pub async fn resolve_pack(&self, manifest: &Manifest) -> Result<crate::ResolvedPack, Error> {
    let ids: Vec<u32> = manifest.files.iter().map(|f| f.file_id).collect();
    let fetched = self.get_files(&ids).await?;
    let mut by_id = std::collections::HashMap::new();
    for f in fetched {
        by_id.insert(f.id, f);
    }
    let mut files = Vec::new();
    for row in &manifest.files {
        match by_id.remove(&row.file_id) {
            Some(file) => files.push(crate::ResolvedPackFile {
                project_id: row.project_id,
                file_id: row.file_id,
                required: row.required,
                file,
            }),
            None if row.required => {
                return Err(Error::NotFound {
                    kind: crate::ResourceKind::File,
                    id: row.file_id,
                });
            }
            None => {}
        }
    }
    Ok(crate::ResolvedPack {
        manifest: manifest.clone(),
        files,
    })
}

pub async fn resolve_required_deps(
    &self,
    roots: &[File],
    game_version: &str,
    loader: Option<crate::ModLoaderType>,
) -> Result<Vec<File>, Error> {
    use std::collections::{HashSet, VecDeque};
    let mut seen: HashSet<u32> = roots.iter().map(|f| f.mod_id).collect();
    let mut queue: VecDeque<u32> = roots
        .iter()
        .flat_map(|f| f.required_mod_ids())
        .filter(|id| !seen.contains(id))
        .collect();
    let mut out = Vec::new();
    while let Some(mod_id) = queue.pop_front() {
        if !seen.insert(mod_id) {
            continue;
        }
        if out.len() >= 512 {
            return Err(Error::InvalidQuery {
                message: "dependency walk exceeded 512",
            });
        }
        let m = self.get_mod(mod_id).await?;
        let idx = m.file_index_for(game_version, loader).ok_or_else(|| {
            Error::NoCompatibleFile {
                mod_id,
                game_version: game_version.to_string(),
            }
        })?;
        let file = self.get_file(mod_id, idx.file_id).await?;
        for dep in file.required_mod_ids() {
            if !seen.contains(&dep) {
                queue.push_back(dep);
            }
        }
        out.push(file);
    }
    Ok(out)
}

pub async fn fingerprints(
    &self,
    fingerprints: &[u32],
) -> Result<crate::FingerprintMatches, Error> {
    if fingerprints.is_empty() {
        return Ok(crate::FingerprintMatches {
            exact: Vec::new(),
            unmatched: Vec::new(),
        });
    }
    #[derive(serde::Serialize)]
    struct Body<'a> {
        fingerprints: &'a [u32],
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Data {
        #[serde(default)]
        exact_matches: Vec<Exact>,
        #[serde(default)]
        unmatched_fingerprints: Vec<u32>,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Exact {
        id: u32,
        file: File,
        #[serde(default)]
        latest_files: Vec<File>,
    }
    let data: Data = self
        .post_data("/v1/fingerprints/432", &Body { fingerprints })
        .await?;
    Ok(crate::FingerprintMatches {
        exact: data
            .exact_matches
            .into_iter()
            .map(|e| crate::FingerprintMatch {
                id: e.id,
                file: e.file,
                latest_files: e.latest_files,
            })
            .collect(),
        unmatched: data.unmatched_fingerprints,
    })
}
```

Fix the seen/queue logic: spec says insert into `seen` immediately when popping, then fetch. Roots are already in `seen`, so queued ids are not. On pop, `seen.insert` is true for new ids. The `if !seen.insert { continue }` handles duplicates in the queue. Cap: count fetched files (`out.len()` before push, fail when about to exceed 512). Spec: "Cap the walk at 512 fetched files. Above that: InvalidQuery". After 512 successful fetches, the next one errors. Check `if out.len() >= 512` **before** fetch, as written.

`lib.rs` re-export `ResolvedPack`, `ResolvedPackFile`, `FingerprintMatches`, `FingerprintMatch`.

`ModLinks`: add `#[serde(default)]` on the struct and all fields so `{}` works.

`Manifest` already `Clone` from Task 3.

- [ ] **Step 4: Run tests**

Run: `cargo test -p kmine-curseforge`

Expected: PASS, including extract tests.

If slim `Mod` JSON fails, add `#[serde(default)]` on `ModLinks` fields (`Option<String>` already default) and on `Mod.links`.

- [ ] **Step 5: Final crate check**

Run:

```bash
cargo test -p kmine-curseforge
cargo check -p kmine
```

`kmine` / `kmine-engine` must still compile and must **not** gain a `kmine-curseforge` dependency.

Confirm `lib.rs` public surface:

```rust
pub use client::{Client, ClientBuilder};
pub use download::{cdn_file_url, resolve_download_url, Downloaded};
pub use error::{Error, ResourceKind};
pub use extract::{CfCoreKey, CfKeyError, extract_from_bytes, extract_from_path};
pub use fetch::{extract_from_source, extract_from_url, LATEST_MAC_DMG};
pub use fingerprint::{fingerprint, FingerprintMatch, FingerprintMatches};
pub use manifest::{
    Manifest, ManifestFile, ManifestLoader, ManifestMinecraft, PackOverride, PackZip,
    ResolvedPack, ResolvedPackFile,
};
pub use search::{CategoryFilter, FileFilter, SearchQuery};
pub use types::*;
```

- [ ] **Step 6: Commit**

```bash
git add crates/curseforge/src/client.rs crates/curseforge/src/manifest.rs crates/curseforge/src/fingerprint.rs crates/curseforge/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(curseforge): resolve packs, required deps, and fingerprints

EOF
)"
```

---

## Self-review

**Spec coverage**

| Spec item | Task |
|---|---|
| Caller-supplied key, empty key error | 4 |
| No disk writes in catalog | 9, 10 (Bytes only) |
| `gameId` 432 constant | 1, 4, 5 |
| Categories + filters | 4 |
| Search + JSON array query + limits | 5 |
| get_mod / get_mods / description + 404 map | 6 |
| files / get_file / get_files chunk 100 / changelog | 7 |
| minecraft versions + loaders | 8 |
| Download + CDN split + SHA-1 + no key on CDN | 9 |
| Manifest parse + type check | 3 |
| PackZip + wrapped folder | 10 |
| resolve_pack required/optional | 11 |
| resolve_required_deps BFS + 512 cap | 11 |
| fingerprint golden vectors | 2 |
| POST `/v1/fingerprints/432` | 11 |
| Types / enums / helpers / fixtures | 1 |
| Retries 503 yes, 404 no | 4 |
| Client Debug hides key | 4 |
| Extract/`cf-key` unchanged | all (do not touch) |
| No engine/UI wiring | all |
| v2 search and get-mod | 5, 6 |

Out of spec on purpose: GPUI store, engine install, Bearer, share-codes, live API tests.

**Placeholder scan:** none. Retry-After integer seconds is specified in Task 4.

**Type consistency:** `SearchQuery`, `FileFilter`, `CategoryFilter` live in `search.rs`. `Downloaded` in `download.rs`. `ResolvedPack*` in `manifest.rs`. `FingerprintMatches` in `fingerprint.rs`. `Client` methods match the spec signatures. `BATCH_SIZE = 100`. `ResourceKind::{Mod, File}`. 404 mapping matches the spec (mod/description → Mod; file/changelog → File; version/loader → Http).
