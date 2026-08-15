use crate::error::EngineError;
use crate::http::{DownloadJob, HttpFiles};
use crate::paths::LauncherPaths;
use crate::types::ProgressSink;
use serde::Deserialize;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

const MOJANG_ASSET_OBJECTS: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetsRoot {
    Objects { dir: PathBuf, index: String },
    Virtual(PathBuf),
    Resources(PathBuf),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetIndexFile {
    #[serde(default)]
    objects: HashMap<String, AssetObject>,
    #[serde(default)]
    map_to_resources: bool,
    #[serde(default)]
    r#virtual: bool,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    #[serde(default)]
    size: Option<u64>,
}

pub async fn fetch_assets(
    http: &HttpFiles,
    paths: &LauncherPaths,
    index_url: &str,
    index_sha1: &str,
    index_id: &str,
    game_dir: &Path,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<AssetsRoot, EngineError> {
    let index_path = paths.cache_assets_indexes.join(format!("{index_id}.json"));
    let expected = if index_sha1.is_empty() {
        None
    } else {
        Some(index_sha1)
    };
    http.download_sha1(index_url, &index_path, expected, cancel)
        .await?;
    let bytes = std::fs::read(&index_path).map_err(|e| EngineError::io(&index_path, e))?;
    let index: AssetIndexFile = serde_json::from_slice(&bytes)
        .map_err(|e| EngineError::io(&index_path, io::Error::other(e.to_string())))?;

    let mut jobs = Vec::with_capacity(index.objects.len());
    let mut copies = Vec::new();
    for (name, object) in &index.objects {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let hash = object.hash.to_ascii_lowercase();
        if hash.len() < 2 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(EngineError::io(
                &index_path,
                io::Error::new(io::ErrorKind::InvalidData, "invalid asset hash"),
            ));
        }
        let dest = crate::paths::safe_join(
            &paths.cache_assets_objects,
            &format!("{}/{}", &hash[..2], hash),
        )?;
        jobs.push(DownloadJob {
            url: object_url(index_url, &hash),
            dest: dest.clone(),
            sha1: Some(hash),
            size: object.size.filter(|size| *size > 0),
        });
        if index.map_to_resources {
            copies.push((
                dest,
                crate::paths::safe_join(&game_dir.join("resources"), name)?,
            ));
        } else if index.r#virtual {
            copies.push((
                dest,
                crate::paths::safe_join(&paths.cache_assets_virtual, name)?,
            ));
        }
    }
    http.download_many(jobs, "Assets", progress, cancel).await?;
    for (src, dest) in copies {
        materialize(&src, &dest)?;
    }

    if index.map_to_resources {
        Ok(AssetsRoot::Resources(game_dir.join("resources")))
    } else if index.r#virtual {
        Ok(AssetsRoot::Virtual(paths.cache_assets_virtual.clone()))
    } else {
        Ok(AssetsRoot::Objects {
            dir: paths.cache_assets_objects.clone(),
            index: index_id.to_string(),
        })
    }
}

fn object_url(index_url: &str, hash: &str) -> String {
    let prefix = &hash[..2];
    let base = if is_official_mojang_index(index_url) {
        MOJANG_ASSET_OBJECTS.to_string()
    } else {
        origin_of(index_url).unwrap_or_else(|| MOJANG_ASSET_OBJECTS.to_string())
    };
    format!("{base}/{prefix}/{hash}")
}

fn is_official_mojang_index(url: &str) -> bool {
    origin_of(url)
        .is_some_and(|origin| origin.contains("mojang.com") || origin.contains("minecraft.net"))
}

fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        ("https", rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        ("http", rest)
    } else {
        return None;
    };
    let host = rest.split('/').next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}"))
}

fn materialize(src: &Path, dest: &Path) -> Result<(), EngineError> {
    if dest.exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
    }
    match std::fs::hard_link(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => std::fs::copy(src, dest)
            .map(|_| ())
            .map_err(|e| EngineError::io(dest, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::fetch_assets;
    use crate::http::HttpFiles;
    use crate::paths::LauncherPaths;
    use crate::types::ProgressSink;
    use sha1::{Digest, Sha1};
    use tokio_util::sync::CancellationToken;

    struct NoopProgress;

    impl ProgressSink for NoopProgress {
        fn set(&self, _title: &str, _done: u64, _total: u64) {}
    }

    #[tokio::test]
    async fn fetch_assets_uses_object_hash_layout() {
        let index_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/assets_index.json");
        let index_bytes = std::fs::read(&index_path).unwrap();
        let index_sha1 = hex::encode(Sha1::digest(&index_bytes));
        let object = b"obj-211";
        let hash = "ab61579307cecea434e3edbe246a9480db728e8a";

        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/index.json"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_raw(index_bytes.clone(), "application/json"),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(format!(
                "/{}/{}",
                &hash[..2],
                hash
            )))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_raw(object.as_slice(), "application/octet-stream"),
            )
            .mount(&server)
            .await;

        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let game_dir = root.path().join("game");
        std::fs::create_dir_all(&game_dir).unwrap();

        fetch_assets(
            &HttpFiles::new().unwrap(),
            &paths,
            &format!("{}/index.json", server.uri()),
            &index_sha1,
            "legacy",
            &game_dir,
            &NoopProgress,
            &CancellationToken::new(),
        )
        .await
        .unwrap();

        let dest =
            crate::paths::safe_join(&paths.cache_assets_objects, &format!("ab/{hash}")).unwrap();
        assert!(dest.is_file(), "missing {dest:?}");
        assert_eq!(std::fs::read(&dest).unwrap(), object);
        let rendered = dest.to_string_lossy();
        assert!(
            rendered.contains("objects/ab/") || rendered.contains("objects\\ab\\"),
            "{rendered}"
        );
    }

    #[test]
    fn asset_relative_names_cannot_escape_cache() {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::paths::LauncherPaths::new(root.path().to_path_buf());
        assert!(crate::paths::safe_join(&paths.cache_assets_virtual, "minecraft/foo.png").is_ok());
        assert!(crate::paths::safe_join(&paths.cache_assets_virtual, "../escape").is_err());
        assert!(crate::paths::safe_join(&paths.cache_assets_objects, "../../etc/passwd").is_err());
    }
}
