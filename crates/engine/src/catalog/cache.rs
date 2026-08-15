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
}
