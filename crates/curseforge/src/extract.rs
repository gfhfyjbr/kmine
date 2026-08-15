use super::asar::{find_asars, Asar};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

const KEY_RE: &str = r"\$2a\$10\$[A-Za-z0-9./]{53}";
const MAX_WALK_DEPTH: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum CfKeyError {
    #[error("no CurseForge Core API key found in {0}")]
    NotFound(String),
    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("http {url}: {message}")]
    Http { url: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfCoreKey {
    pub key: String,
    /// `asar:dist/background/background.js`, `zip:app.asar`, `scan`, …
    pub source: String,
}

pub fn extract_from_path(path: impl AsRef<Path>) -> Result<CfCoreKey, CfKeyError> {
    let path = path.as_ref();
    if path.is_dir() {
        for asar in find_asar_files(path, 0) {
            let bytes = std::fs::read(&asar).map_err(|source| CfKeyError::Io {
                path: asar.clone(),
                source,
            })?;
            if let Some(found) = extract_from_bytes(&bytes) {
                return Ok(found);
            }
        }
        return Err(CfKeyError::NotFound(path.display().to_string()));
    }
    let bytes = std::fs::read(path).map_err(|source| CfKeyError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    extract_from_bytes(&bytes).ok_or_else(|| CfKeyError::NotFound(path.display().to_string()))
}

pub fn extract_from_bytes(bytes: &[u8]) -> Option<CfCoreKey> {
    if let Some(found) = extract_from_asars(bytes) {
        return Some(found);
    }
    if looks_like_zip(bytes)
        && let Some(found) = extract_from_zip(bytes)
    {
        return Some(found);
    }
    if crate::dmg::looks_like_dmg(bytes) {
        return crate::dmg::extract_from_dmg(bytes);
    }
    scan_slice(bytes, "scan")
}

fn extract_from_asars(bytes: &[u8]) -> Option<CfCoreKey> {
    for asar in find_asars(bytes) {
        if let Some(found) = extract_from_asar(&asar) {
            return Some(found);
        }
    }
    None
}

fn extract_from_asar(asar: &Asar<'_>) -> Option<CfCoreKey> {
    let mut files: Vec<(&str, &[u8])> = asar.files().collect();
    files.sort_by_key(|(path, _)| js_rank(path));
    for (path, data) in files {
        if let Some(key) = first_key(data) {
            return Some(CfCoreKey {
                key,
                source: format!("asar:{path}"),
            });
        }
    }
    None
}

fn extract_from_zip(bytes: &[u8]) -> Option<CfCoreKey> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();
    names.sort_by_key(|n| js_rank(n));
    for name in names {
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".asar") && !lower.ends_with(".js") {
            continue;
        }
        let mut file = zip.by_name(&name).ok()?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).ok()?;
        if lower.ends_with(".asar") {
            if let Some(mut found) = extract_from_asars(&buf) {
                found.source = format!("zip:{name}/{}", found.source);
                return Some(found);
            }
        } else if let Some(key) = first_key(&buf) {
            return Some(CfCoreKey {
                key,
                source: format!("zip:{name}"),
            });
        }
    }
    None
}

fn scan_slice(bytes: &[u8], source: &str) -> Option<CfCoreKey> {
    first_key(bytes).map(|key| CfCoreKey {
        key,
        source: source.to_string(),
    })
}

pub(crate) fn first_key(bytes: &[u8]) -> Option<String> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(KEY_RE).expect("key regex"));
    let haystack = std::str::from_utf8(bytes)
        .map(std::borrow::Cow::Borrowed)
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes));
    RE.find(haystack.as_ref()).map(|m| m.as_str().to_string())
}

fn js_rank(path: &str) -> u8 {
    let p = path.replace('\\', "/");
    if p.ends_with("dist/background/background.js") || p.ends_with("/background/background.js") {
        0
    } else if p.ends_with("background.js") {
        1
    } else if p.ends_with(".js") {
        2
    } else if p.ends_with("app.asar") {
        3
    } else {
        4
    }
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}

fn find_asar_files(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if depth > MAX_WALK_DEPTH {
        return out;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(find_asar_files(&path, depth + 1));
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case("app.asar") || n.ends_with(".asar"))
        {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asar::pack;

    const SAMPLE: &str = "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm";

    #[test]
    fn pulls_key_from_asar_js() {
        let js = format!(r#"let tg="{SAMPLE}";this.eleriumApi.setCfCoreApiKey(tg)"#);
        let bytes = pack(&[("dist/background/background.js", js.as_bytes())]);
        let found = extract_from_bytes(&bytes).expect("key");
        assert_eq!(found.key, SAMPLE);
        assert_eq!(found.source, "asar:dist/background/background.js");
    }

    #[test]
    fn pulls_key_from_asar_hidden_in_blob() {
        let js = format!("xx{SAMPLE}yy");
        let inner = pack(&[("foo.js", js.as_bytes())]);
        let mut blob = vec![0u8; 64];
        blob.extend_from_slice(&inner);
        blob.extend_from_slice(&[1, 2, 3]);
        let found = extract_from_bytes(&blob).expect("key");
        assert_eq!(found.key, SAMPLE);
    }

    #[test]
    fn raw_scan_without_asar() {
        let blob = format!("noise {SAMPLE} noise").into_bytes();
        let found = extract_from_bytes(&blob).expect("key");
        assert_eq!(found.key, SAMPLE);
        assert_eq!(found.source, "scan");
    }

    #[test]
    fn real_client_app_if_checked_in() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../CurseForge.app");
        if !path.exists() {
            return;
        }
        let found = extract_from_path(&path).expect("key in CurseForge.app");
        assert_eq!(found.key, SAMPLE);
        assert!(found.source.contains("background.js"), "{}", found.source);
    }
}
