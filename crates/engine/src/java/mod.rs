mod platform;

pub use platform::platform_id;

use crate::error::EngineError;
use crate::http::{DownloadJob, HttpFiles};
use crate::mojang::VersionInfo;
use crate::paths::LauncherPaths;
use crate::types::{PrepareMode, ProgressSink};
use serde::Deserialize;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

const JAVA_RUNTIME_ALL_JSON: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

pub fn find_java_binary(hint: &Path) -> Option<PathBuf> {
    if hint.is_file() {
        match hint.file_name().and_then(|n| n.to_str()) {
            Some("java") | Some("java.exe") => return Some(hint.to_path_buf()),
            _ => {}
        }
    }
    for rel in [
        ["bin", "java"].as_slice(),
        ["bin", "java.exe"].as_slice(),
        ["Contents", "Home", "bin", "java"].as_slice(),
    ] {
        let mut candidate = hint.to_path_buf();
        for part in rel {
            candidate.push(*part);
        }
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub async fn resolve_java(
    http: &HttpFiles,
    paths: &LauncherPaths,
    version: &VersionInfo,
    custom: Option<&Path>,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
    mode: PrepareMode,
) -> Result<PathBuf, EngineError> {
    resolve_java_from(
        http,
        paths,
        version,
        custom,
        progress,
        cancel,
        JAVA_RUNTIME_ALL_JSON,
        mode,
    )
    .await
}

async fn resolve_java_from(
    http: &HttpFiles,
    paths: &LauncherPaths,
    version: &VersionInfo,
    custom: Option<&Path>,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
    all_json_url: &str,
    mode: PrepareMode,
) -> Result<PathBuf, EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    if let Some(hint) = custom {
        return find_java_binary(hint).ok_or(EngineError::JavaNotFound);
    }

    let component = version
        .java_version
        .as_ref()
        .map(|j| j.component.as_str())
        .filter(|c| !c.is_empty())
        .unwrap_or("jre-legacy");
    let platform = platform_id(std::env::consts::OS, std::env::consts::ARCH);

    let all_path = paths.cache_meta.join("java-all.json");
    let all: JavaAll = http
        .load_meta_json(all_json_url, &all_path, mode, cancel)
        .await?;

    let (used_platform, entry) =
        pick_runtime(&all, &platform, component).ok_or(EngineError::JavaNotFound)?;

    let manifest_path = paths
        .cache_meta
        .join(format!("java-{component}-{used_platform}.json"));
    http.download_sha1(
        &entry.manifest.url,
        &manifest_path,
        entry.manifest.sha1.as_deref(),
        entry.manifest.size,
        cancel,
        mode,
    )
    .await?;
    let manifest_bytes =
        std::fs::read(&manifest_path).map_err(|e| EngineError::io(&manifest_path, e))?;
    let manifest: FileManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| EngineError::io(&manifest_path, io::Error::other(e.to_string())))?;

    let dest_root = paths.cache_runtime.join(component).join(used_platform);
    std::fs::create_dir_all(&dest_root).map_err(|e| EngineError::io(&dest_root, e))?;

    let mut jobs = Vec::new();
    let mut executables = Vec::new();
    let mut links = Vec::new();
    for (rel, file) in &manifest.files {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let dest = dest_for(&dest_root, rel)?;
        match file {
            RuntimeFile::Directory => {
                std::fs::create_dir_all(&dest).map_err(|e| EngineError::io(&dest, e))?;
            }
            RuntimeFile::File {
                downloads,
                executable,
            } => {
                let raw = downloads.raw.as_ref().ok_or_else(|| {
                    EngineError::io(
                        &dest,
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "runtime file missing raw download",
                        ),
                    )
                })?;
                jobs.push(DownloadJob {
                    url: raw.url.clone(),
                    dest: dest.clone(),
                    sha1: raw.sha1.clone(),
                    size: raw.size.filter(|size| *size > 0),
                });
                if *executable {
                    executables.push(dest);
                }
            }
            RuntimeFile::Link { target } => links.push((dest, target.clone())),
        }
    }
    http.download_many(jobs, "Java", progress, cancel, mode)
        .await?;
    for dest in executables {
        set_executable(&dest)?;
    }
    for (dest, target) in links {
        create_link(&dest, &target)?;
    }

    locate_runtime_java(&dest_root).ok_or(EngineError::JavaNotFound)
}

fn pick_runtime<'a>(
    all: &'a JavaAll,
    platform: &'a str,
    component: &str,
) -> Option<(&'a str, &'a RuntimeEntry)> {
    if let Some(entry) = component_entry(all, platform, component) {
        return Some((platform, entry));
    }
    if platform == "mac-os-arm64" {
        if let Some(entry) = component_entry(all, "mac-os", component) {
            return Some(("mac-os", entry));
        }
    }
    None
}

fn component_entry<'a>(
    all: &'a JavaAll,
    platform: &str,
    component: &str,
) -> Option<&'a RuntimeEntry> {
    all.get(platform)
        .and_then(|comps| comps.get(component))
        .and_then(|arr| arr.first())
        .filter(|entry| !entry.manifest.url.is_empty())
}

fn locate_runtime_java(root: &Path) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        for rel in [
            ["Contents", "Home", "bin", "java"].as_slice(),
            ["jre.bundle", "Contents", "Home", "bin", "java"].as_slice(),
        ] {
            let mut mac = root.to_path_buf();
            for part in rel {
                mac.push(*part);
            }
            if mac.is_file() {
                return Some(mac);
            }
        }
    }
    let unix = root.join("bin").join("java");
    if unix.is_file() {
        return Some(unix);
    }
    let win = root.join("bin").join("java.exe");
    if win.is_file() {
        return Some(win);
    }
    None
}

fn dest_for(root: &Path, rel: &str) -> Result<PathBuf, EngineError> {
    crate::paths::safe_join(root, rel)
}

fn set_executable(path: &Path) -> Result<(), EngineError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| EngineError::io(path, e))?;
    }
    let _ = path;
    Ok(())
}

fn create_link(dest: &Path, target: &str) -> Result<(), EngineError> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
        }
    }
    match dest.symlink_metadata() {
        Ok(meta) if meta.file_type().is_dir() => {
            std::fs::remove_dir_all(dest).map_err(|e| EngineError::io(dest, e))?;
        }
        Ok(_) => {
            std::fs::remove_file(dest).map_err(|e| EngineError::io(dest, e))?;
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(EngineError::io(dest, err)),
    }
    #[cfg(unix)]
    {
        return std::os::unix::fs::symlink(target, dest).map_err(|e| EngineError::io(dest, e));
    }
    #[cfg(windows)]
    {
        return std::os::windows::fs::symlink_file(target, dest)
            .or_else(|_| std::os::windows::fs::symlink_dir(target, dest))
            .map_err(|e| EngineError::io(dest, e));
    }
    #[allow(unreachable_code)]
    {
        let _ = target;
        Err(EngineError::io(
            dest,
            io::Error::new(io::ErrorKind::Unsupported, "symlinks not supported"),
        ))
    }
}

type JavaAll = HashMap<String, HashMap<String, Vec<RuntimeEntry>>>;

#[derive(Debug, Deserialize)]
struct RuntimeEntry {
    manifest: RemoteFile,
}

#[derive(Debug, Deserialize)]
struct RemoteFile {
    sha1: Option<String>,
    url: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FileManifest {
    #[serde(default)]
    files: HashMap<String, RuntimeFile>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum RuntimeFile {
    #[serde(rename = "file")]
    File {
        downloads: FileDownloads,
        #[serde(default)]
        executable: bool,
    },
    #[serde(rename = "directory")]
    Directory,
    #[serde(rename = "link")]
    Link { target: String },
}

#[derive(Debug, Deserialize)]
struct FileDownloads {
    raw: Option<RemoteFile>,
}

#[cfg(test)]
mod tests {
    use super::{find_java_binary, platform_id, resolve_java, resolve_java_from};
    use crate::error::EngineError;
    use crate::http::HttpFiles;
    use crate::mojang::VersionInfo;
    use crate::paths::LauncherPaths;
    use crate::types::{PrepareMode, ProgressSink};
    use sha1::{Digest, Sha1};
    use std::path::Path;
    use tokio_util::sync::CancellationToken;

    struct NoopProgress;

    impl ProgressSink for NoopProgress {
        fn set(&self, _title: &str, _done: u64, _total: u64) {}
    }

    fn sha1_hex(bytes: &[u8]) -> String {
        hex::encode(Sha1::digest(bytes))
    }

    fn sample_version() -> VersionInfo {
        serde_json::from_str(
            r#"{
                "id": "1.21.1",
                "mainClass": "net.minecraft.client.main.Main",
                "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 }
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn platform_id_macos_arm() {
        assert_eq!(platform_id("macos", "aarch64"), "mac-os-arm64");
        assert_eq!(platform_id("linux", "x86_64"), "linux");
        assert_eq!(platform_id("linux", "x86"), "linux-i386");
        assert_eq!(platform_id("macos", "x86_64"), "mac-os");
        assert_eq!(platform_id("windows", "x86_64"), "windows-x64");
        assert_eq!(platform_id("windows", "aarch64"), "windows-arm64");
        assert_eq!(platform_id("windows", "x86"), "windows-x86");
        assert_eq!(platform_id("freebsd", "x86_64"), "freebsd-x86_64");
    }

    #[test]
    fn find_java_binary_accepts_bin_java() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let java = bin.join("java");
        std::fs::write(&java, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(
            find_java_binary(dir.path()).as_deref(),
            Some(java.as_path())
        );
    }

    #[tokio::test]
    async fn resolve_custom_missing_errors() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let err = resolve_java(
            &HttpFiles::new().unwrap(),
            &paths,
            &sample_version(),
            Some(Path::new("/no/java/here")),
            &NoopProgress,
            &CancellationToken::new(),
            PrepareMode::Warm,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, EngineError::JavaNotFound));
    }

    #[tokio::test]
    async fn install_runtime_downloads_bin_java() {
        let java_bytes = b"fake-java";
        let java_sha1 = sha1_hex(java_bytes);
        let server = wiremock::MockServer::start().await;
        let platform = platform_id(std::env::consts::OS, std::env::consts::ARCH);
        let manifest = serde_json::json!({
            "files": {
                "bin/java": {
                    "type": "file",
                    "executable": true,
                    "downloads": {
                        "raw": {
                            "sha1": java_sha1,
                            "size": java_bytes.len(),
                            "url": format!("{}/java", server.uri())
                        }
                    }
                }
            }
        });
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let manifest_sha1 = sha1_hex(&manifest_bytes);
        let all = serde_json::json!({
            platform.clone(): {
                "java-runtime-delta": [{
                    "manifest": {
                        "sha1": manifest_sha1,
                        "size": manifest_bytes.len(),
                        "url": format!("{}/manifest.json", server.uri())
                    }
                }]
            }
        });
        let all_bytes = serde_json::to_vec(&all).unwrap();

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/all.json"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_raw(all_bytes.clone(), "application/json"),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/manifest.json"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_raw(manifest_bytes, "application/json"),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/java"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_raw(java_bytes.as_slice(), "application/octet-stream"),
            )
            .mount(&server)
            .await;

        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        // Stale body with old mtime so Warm TTL re-fetches from the mock server.
        let all_path = paths.cache_meta.join("java-all.json");
        std::fs::write(&all_path, b"{\"stale\":true}").unwrap();
        let file = std::fs::File::options()
            .write(true)
            .open(&all_path)
            .unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(4000))
            .unwrap();
        drop(file);
        let java = resolve_java_from(
            &HttpFiles::new().unwrap(),
            &paths,
            &sample_version(),
            None,
            &NoopProgress,
            &CancellationToken::new(),
            &format!("{}/all.json", server.uri()),
            PrepareMode::Warm,
        )
        .await
        .unwrap();

        let dest = paths
            .cache_runtime
            .join("java-runtime-delta")
            .join(&platform)
            .join("bin")
            .join("java");
        assert!(dest.is_file(), "missing {dest:?}");
        assert_eq!(std::fs::read(&dest).unwrap(), java_bytes);
        assert_eq!(java, dest);
    }
}
