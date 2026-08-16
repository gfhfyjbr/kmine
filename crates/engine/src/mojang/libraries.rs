use super::rules::{FeatureSet, current_os_name, rule_allows};
use super::{Artifact, VersionInfo};
use crate::error::EngineError;
use crate::http::{DownloadJob, HttpFiles};
use crate::paths::LauncherPaths;
use crate::types::{PrepareMode, ProgressSink};
use sha1::{Digest, Sha1};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryArtifact {
    pub path: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub extract_natives: bool,
}

pub fn select_libraries(version: &VersionInfo) -> Vec<LibraryArtifact> {
    let mut out = Vec::new();
    for lib in &version.libraries {
        if !rule_allows(&lib.rules, &FeatureSet::default()) {
            continue;
        }
        let Some(downloads) = &lib.downloads else {
            continue;
        };
        if let Some(artifact) = downloads
            .artifact
            .as_ref()
            .and_then(|a| from_download(a, false))
        {
            out.push(artifact);
        }
        if lib
            .natives
            .as_ref()
            .is_some_and(|natives| natives.contains_key(current_os_name()))
        {
            if let Some(art) = downloads
                .classifiers
                .as_ref()
                .and_then(|c| c.get(legacy_natives_classifier()))
                .and_then(|a| from_download(a, true))
            {
                out.push(art);
            }
        }
    }
    out
}

pub async fn fetch_libraries(
    http: &HttpFiles,
    paths: &LauncherPaths,
    artifacts: &[LibraryArtifact],
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
    mode: PrepareMode,
) -> Result<Vec<PathBuf>, EngineError> {
    let mut dests = Vec::with_capacity(artifacts.len());
    let mut jobs = Vec::new();
    for artifact in artifacts {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let dest = crate::paths::safe_join(&paths.cache_libraries, &artifact.path)?;
        if artifact.url.is_empty() {
            if dest.is_file() {
                dests.push(dest);
                continue;
            }
            return Err(EngineError::io(
                dest,
                io::Error::new(io::ErrorKind::NotFound, "library missing (no download url)"),
            ));
        }
        jobs.push(DownloadJob {
            url: artifact.url.clone(),
            dest: dest.clone(),
            sha1: artifact.sha1.clone(),
            size: artifact.size.filter(|size| *size > 0),
        });
        dests.push(dest);
    }
    http.download_many(jobs, "Libraries", progress, cancel, mode)
        .await?;
    Ok(dests)
}

pub fn natives_dir_name(artifacts: &[LibraryArtifact], sandbox: bool) -> String {
    let mut paths: Vec<&str> = artifacts.iter().map(|a| a.path.as_str()).collect();
    paths.sort_unstable();
    let mut hasher = Sha1::new();
    for path in paths {
        hasher.update(path.as_bytes());
        hasher.update(b"\n");
    }
    let hex = hex::encode(hasher.finalize());
    if sandbox {
        format!("{hex}-sandbox")
    } else {
        hex
    }
}

pub fn extract_natives(jar: &Path, dest: &Path, exclude: &[String]) -> Result<(), EngineError> {
    let file = std::fs::File::open(jar).map_err(|e| EngineError::io(jar, e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?;
    std::fs::create_dir_all(dest).map_err(|e| EngineError::io(dest, e))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let name = rel.to_string_lossy().replace('\\', "/");
        if skip_native_entry(&name, exclude) {
            continue;
        }
        let out = dest.join(rel);
        if !out.starts_with(dest) {
            continue;
        }
        if entry.is_dir() || name.ends_with('/') {
            std::fs::create_dir_all(&out).map_err(|e| EngineError::io(&out, e))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
        }
        let mut outfile = std::fs::File::create(&out).map_err(|e| EngineError::io(&out, e))?;
        io::copy(&mut entry, &mut outfile).map_err(|e| EngineError::io(&out, e))?;
        outfile.flush().map_err(|e| EngineError::io(&out, e))?;
    }
    Ok(())
}

pub async fn fetch_client(
    http: &HttpFiles,
    paths: &LauncherPaths,
    version: &VersionInfo,
    cancel: &CancellationToken,
    mode: PrepareMode,
) -> Result<PathBuf, EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    let Some(client) = version
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.client.as_ref())
    else {
        return Err(EngineError::io(
            paths.cache_libraries.clone(),
            io::Error::new(io::ErrorKind::NotFound, "client download missing"),
        ));
    };
    let dest = paths
        .cache_libraries
        .join("com")
        .join("mojang")
        .join("minecraft")
        .join(&version.id)
        .join(format!("minecraft-{}-client.jar", version.id));
    http.download_sha1(&client.url, &dest, Some(&client.sha1), cancel, mode)
        .await?;
    Ok(dest)
}

fn from_download(art: &Artifact, extract_natives: bool) -> Option<LibraryArtifact> {
    let path = art.path.as_ref().filter(|p| !p.is_empty())?.clone();
    let url = art.url.clone().unwrap_or_default();
    Some(LibraryArtifact {
        path,
        url,
        sha1: art.sha1.as_ref().filter(|s| !s.is_empty()).cloned(),
        size: art.size,
        extract_natives,
    })
}

fn legacy_natives_classifier() -> &'static str {
    match current_os_name() {
        "osx" => "natives-osx",
        "linux" => "natives-linux",
        "windows" => "natives-windows",
        other => other,
    }
}

fn skip_native_entry(name: &str, exclude: &[String]) -> bool {
    let name = name.trim_start_matches('/');
    if name == "META-INF" || name.starts_with("META-INF/") {
        return true;
    }
    exclude.iter().any(|prefix| name.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::{LibraryArtifact, extract_natives, natives_dir_name, select_libraries};
    use crate::mojang::VersionInfo;
    use crate::mojang::rules::current_os_name;
    use std::io::Write;

    fn load_fixture(name: &str) -> VersionInfo {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let text = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn artifact(path: &str) -> LibraryArtifact {
        LibraryArtifact {
            path: path.into(),
            url: format!("https://example.invalid/{path}"),
            sha1: None,
            size: None,
            extract_natives: false,
        }
    }

    #[test]
    fn select_libraries_skips_disallowed() {
        let version = load_fixture("version_libs.json");
        let libs = select_libraries(&version);
        assert!(
            libs.iter()
                .any(|lib| lib.path == "com/example/allowed/1.0/allowed-1.0.jar")
        );
        let has_nolinux = libs.iter().any(|lib| lib.path.contains("nolinux"));
        if current_os_name() == "linux" {
            assert!(!has_nolinux);
            assert_eq!(libs.len(), 1);
        } else {
            assert!(has_nolinux);
            assert_eq!(libs.len(), 2);
        }
    }

    #[test]
    fn extract_natives_skips_meta_inf() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("natives.jar");
        let file = std::fs::File::create(&jar).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("a.so", options).unwrap();
        zip.write_all(b"native").unwrap();
        zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
        zip.write_all(b"Manifest-Version: 1.0\n").unwrap();
        zip.finish().unwrap();

        let dest = dir.path().join("out");
        extract_natives(&jar, &dest, &[]).unwrap();
        assert!(dest.join("a.so").is_file());
        assert!(!dest.join("META-INF").exists());
    }

    #[tokio::test]
    async fn fetch_libraries_rejects_parent_path() {
        use crate::http::HttpFiles;
        use crate::paths::LauncherPaths;
        use crate::types::ProgressSink;
        use tokio_util::sync::CancellationToken;

        struct Noop;
        impl ProgressSink for Noop {
            fn set(&self, _title: &str, _done: u64, _total: u64) {}
        }

        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let err = super::fetch_libraries(
            &HttpFiles::new().unwrap(),
            &paths,
            &[artifact("../escape.jar")],
            &Noop,
            &CancellationToken::new(),
            crate::types::PrepareMode::Warm,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, crate::error::EngineError::Io { .. }));
    }

    #[test]
    fn natives_dir_name_stable_and_sandbox_suffix() {
        let a = artifact("b.jar");
        let b = artifact("a.jar");
        let name = natives_dir_name(&[a.clone(), b.clone()], false);
        assert_eq!(name, natives_dir_name(&[b.clone(), a.clone()], false));
        assert_eq!(name.len(), 40);
        assert_eq!(natives_dir_name(&[a, b], true), format!("{name}-sandbox"));
    }
}
