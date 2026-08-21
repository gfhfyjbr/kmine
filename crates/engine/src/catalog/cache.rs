use super::provider::ProviderId;
use super::types::{CatalogBlob, CatalogError};
use crate::error::EngineError;
use crate::paths::{LauncherPaths, safe_join};
use std::path::{Path, PathBuf};

pub fn blob_path(
    paths: &LauncherPaths,
    provider: ProviderId,
    file_id: &str,
    file_name: &str,
) -> Result<PathBuf, EngineError> {
    let provider_dir = safe_join(&paths.cache_catalog_files, provider.0)?;
    let file_dir = safe_join(&provider_dir, file_id)?;
    safe_join(&file_dir, file_name)
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

pub(crate) fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    hex::encode(Sha1::digest(bytes))
}

pub(crate) fn image_cache_key(url: &str) -> String {
    sha1_hex(url.as_bytes())
}

fn nonempty_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

pub(crate) fn find_cached_image(dir: &Path, url: &str) -> Option<PathBuf> {
    let hash = image_cache_key(url);
    for ext in [".png", ".jpg", ".jpeg", ".webp", ".gif", ".img"] {
        let path = dir.join(format!("{hash}{ext}"));
        if nonempty_file(&path) {
            return Some(path);
        }
    }
    let prefix = format!("{hash}.");
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(&prefix) && !name.ends_with(".part") {
            let path = entry.path();
            if nonempty_file(&path) {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::provider::ProviderId;
    use crate::paths::LauncherPaths;

    #[test]
    fn blob_path_stays_under_catalog_files() {
        let paths = LauncherPaths::new(PathBuf::from("/tmp/kmine-test"));
        let dest = blob_path(&paths, ProviderId::CURSEFORGE, "2", "jei.jar").unwrap();
        assert!(dest.starts_with(&paths.cache_catalog_files));
        assert_eq!(
            dest,
            paths
                .cache_catalog_files
                .join("curseforge")
                .join("2")
                .join("jei.jar")
        );
    }

    #[test]
    fn blob_path_rejects_escape() {
        let paths = LauncherPaths::new(PathBuf::from("/tmp/kmine-test"));
        let cases = [
            ("../x", "a.jar"),
            ("2", "../escape.jar"),
            ("2", "/tmp/evil.jar"),
            ("/tmp", "a.jar"),
            ("2", "foo/../../evil.jar"),
            ("..", "a.jar"),
        ];
        for (file_id, file_name) in cases {
            let err = blob_path(&paths, ProviderId::CURSEFORGE, file_id, file_name).unwrap_err();
            assert!(
                matches!(err, EngineError::Io { .. }),
                "{file_id}/{file_name}: {err:?}"
            );
        }
        assert!(blob_path(&paths, ProviderId(".."), "2", "a.jar").is_err());
        assert!(blob_path(&paths, ProviderId("/tmp"), "2", "a.jar").is_err());
    }

    #[test]
    fn find_cached_image_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_cached_image(dir.path(), "https://cdn.example/a.png").is_none());
    }

    #[test]
    fn find_cached_image_reads_hashed_file() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://media.forgecdn.net/avatars/a.png";
        let dest = dir.path().join(format!("{}.png", image_cache_key(url)));
        std::fs::write(&dest, b"png").unwrap();
        assert_eq!(
            find_cached_image(dir.path(), url).as_deref(),
            Some(dest.as_path())
        );
    }

    #[test]
    fn find_cached_image_skips_empty_files() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://cdn.example/b.webp";
        let dest = dir.path().join(format!("{}.webp", image_cache_key(url)));
        std::fs::write(&dest, b"").unwrap();
        assert!(find_cached_image(dir.path(), url).is_none());
    }

    #[test]
    fn find_cached_image_finds_unknown_extension() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://cdn.example/c.bin";
        let dest = dir.path().join(format!("{}.xyz", image_cache_key(url)));
        std::fs::write(&dest, b"gif").unwrap();
        assert_eq!(
            find_cached_image(dir.path(), url).as_deref(),
            Some(dest.as_path())
        );
    }
}
