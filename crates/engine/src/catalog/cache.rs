use super::provider::ProviderId;
use super::types::{CatalogBlob, CatalogError};
use crate::paths::LauncherPaths;
use std::path::{Path, PathBuf};

pub fn blob_path(
    paths: &LauncherPaths,
    provider: ProviderId,
    file_id: &str,
    file_name: &str,
) -> PathBuf {
    paths
        .cache_catalog_files
        .join(provider.0)
        .join(file_id)
        .join(file_name)
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
