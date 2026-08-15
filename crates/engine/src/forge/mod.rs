mod processors;

pub use processors::{run_processors, subst_arg};

use crate::error::EngineError;
use crate::http::{DownloadJob, HttpFiles};
use crate::ids::Loader;
use crate::mojang::{Artifact, Library, LibraryDownloads, VersionInfo};
use crate::paths::LauncherPaths;
use crate::types::ProgressSink;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

pub const MAVEN_METADATA_URL: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ForgeInstallProfile {
    #[serde(default)]
    pub spec: i32,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub minecraft: Option<String>,
    #[serde(default)]
    pub data: HashMap<String, ForgeDataFile>,
    #[serde(default)]
    pub processors: Vec<ForgeProcessor>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(skip)]
    pub installer_path: PathBuf,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ForgeDataFile {
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub server: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ForgeProcessor {
    #[serde(default)]
    pub sides: Vec<String>,
    #[serde(default)]
    pub jar: String,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

pub fn pick_forge_version(
    mc: &str,
    versions: &[String],
    preferred: Option<&str>,
) -> Result<String, EngineError> {
    if let Some(preferred) = preferred {
        return Ok(preferred.to_string());
    }
    let prefix = format!("{mc}-");
    versions
        .iter()
        .filter(|version| version.starts_with(&prefix))
        .max_by(|a, b| cmp_forge_version(a, b))
        .cloned()
        .ok_or_else(|| EngineError::LoaderUnavailable {
            loader: Loader::Forge,
            minecraft: mc.to_string(),
        })
}

pub fn installer_url(ver: &str) -> String {
    format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{ver}/forge-{ver}-installer.jar"
    )
}

pub fn parse_maven_versions(xml: &str) -> Result<Vec<String>, EngineError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut versions = Vec::new();
    let mut buf = Vec::new();
    let mut in_version = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"version" => {
                in_version = true;
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == b"version" => {
                in_version = false;
            }
            Ok(Event::Text(t)) if in_version => {
                let text = t.decode().map_err(|err| {
                    EngineError::io(
                        PathBuf::from("maven-metadata.xml"),
                        io::Error::other(err.to_string()),
                    )
                })?;
                if !text.is_empty() {
                    versions.push(text.into_owned());
                }
                in_version = false;
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(EngineError::io(
                    PathBuf::from("maven-metadata.xml"),
                    io::Error::other(err.to_string()),
                ));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(versions)
}

pub fn read_installer(jar: &Path) -> Result<(ForgeInstallProfile, VersionInfo), EngineError> {
    let file = std::fs::File::open(jar).map_err(|e| EngineError::io(jar, e))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?;
    let mut profile: ForgeInstallProfile = read_zip_json(&mut zip, jar, "install_profile.json")?;
    let version: VersionInfo = read_zip_json(&mut zip, jar, "version.json")?;
    profile.installer_path = jar.to_path_buf();
    Ok((profile, version))
}

pub fn merge_forge(mut vanilla: VersionInfo, mut forge_version: VersionInfo) -> VersionInfo {
    vanilla.main_class = forge_version.main_class;
    vanilla.id = forge_version.id;
    for lib in &mut forge_version.libraries {
        ensure_library_artifact(lib);
    }
    vanilla.libraries.append(&mut forge_version.libraries);
    if let Some(forge_args) = forge_version.arguments {
        match vanilla.arguments.as_mut() {
            Some(existing) => {
                existing.game.extend(forge_args.game);
                existing.jvm.extend(forge_args.jvm);
            }
            None => vanilla.arguments = Some(forge_args),
        }
    }
    vanilla
}

pub async fn prepare_forge(
    http: &HttpFiles,
    paths: &LauncherPaths,
    mc: &str,
    preferred: Option<&str>,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<(ForgeInstallProfile, VersionInfo), EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    progress.set("Forge installer", 0, 2);
    let meta_path = paths.cache_meta.join("forge-maven-metadata.xml");
    if meta_path.exists() {
        let _ = std::fs::remove_file(&meta_path);
    }
    http.download_sha1(MAVEN_METADATA_URL, &meta_path, None, cancel)
        .await?;
    let xml = std::fs::read_to_string(&meta_path).map_err(|e| EngineError::io(&meta_path, e))?;
    let versions = parse_maven_versions(&xml)?;
    let ver = match pick_forge_version(mc, &versions, preferred) {
        Ok(ver) => ver,
        Err(EngineError::LoaderUnavailable { loader, .. }) => {
            return Err(EngineError::LoaderUnavailable {
                loader,
                minecraft: mc.to_string(),
            });
        }
        Err(err) => return Err(err),
    };

    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    progress.set("Forge installer", 1, 2);
    let url = installer_url(&ver);
    let installer_path = paths.cache_libraries.join(format!(
        "net/minecraftforge/forge/{ver}/forge-{ver}-installer.jar"
    ));
    let sha1_path = installer_path.with_extension("jar.sha1");
    let sha1 = fetch_installer_sha1(http, &url, &sha1_path, cancel).await?;
    match http
        .download_sha1(&url, &installer_path, sha1.as_deref(), cancel)
        .await
    {
        Err(EngineError::Http { status: 404, .. }) => {
            return Err(EngineError::LoaderUnavailable {
                loader: Loader::Forge,
                minecraft: mc.to_string(),
            });
        }
        other => other?,
    }

    let (profile, forge_version) = read_installer(&installer_path)?;
    progress.set("Forge installer", 2, 2);
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    fetch_installer_libraries(http, paths, &profile, progress, cancel).await?;
    Ok((profile, forge_version))
}

async fn fetch_installer_sha1(
    http: &HttpFiles,
    installer_url: &str,
    dest: &Path,
    cancel: &CancellationToken,
) -> Result<Option<String>, EngineError> {
    let url = format!("{installer_url}.sha1");
    match http.download_sha1(&url, dest, None, cancel).await {
        Ok(()) => {
            let text = std::fs::read_to_string(dest).map_err(|e| EngineError::io(dest, e))?;
            let hash = text
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                Ok(Some(hash))
            } else {
                Ok(None)
            }
        }
        Err(EngineError::Http { status, .. }) if status == 404 || status == 403 => Ok(None),
        Err(err) => Err(err),
    }
}

async fn fetch_installer_libraries(
    http: &HttpFiles,
    paths: &LauncherPaths,
    profile: &ForgeInstallProfile,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<(), EngineError> {
    let mut libs = profile.libraries.clone();
    for lib in &mut libs {
        ensure_library_artifact(lib);
    }
    let mut jobs = Vec::new();
    for lib in &libs {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let Some(art) = lib
            .downloads
            .as_ref()
            .and_then(|d| d.artifact.as_ref())
            .filter(|a| a.path.as_ref().is_some_and(|p| !p.is_empty()))
        else {
            continue;
        };
        let path = art.path.as_deref().unwrap();
        let dest = crate::paths::safe_join(&paths.cache_libraries, path)?;
        let url = art.url.as_deref().unwrap_or("");
        if url.is_empty() {
            if !(dest.is_file() && file_nonempty(&dest)) {
                let entry = format!("maven/{}", path.replace('\\', "/"));
                extract_zip_entry(&profile.installer_path, &entry, &dest)?;
            }
        } else {
            jobs.push(DownloadJob {
                url: url.to_string(),
                dest,
                sha1: art.sha1.clone(),
                size: art.size.filter(|size| *size > 0),
            });
        }
    }
    http.download_many(jobs, "Forge libraries", progress, cancel)
        .await?;
    Ok(())
}

fn ensure_library_artifact(lib: &mut Library) {
    if let Some(downloads) = lib.downloads.as_mut() {
        if let Some(art) = downloads.artifact.as_mut() {
            if art.path.as_ref().is_none_or(|p| p.is_empty()) {
                art.path = maven_path(&lib.name);
            }
            if art.url.as_ref().is_none_or(|u| u.is_empty()) {
                if let (Some(base), Some(path)) = (lib.url.as_deref(), art.path.as_deref()) {
                    art.url = Some(join_maven_url(base, path));
                }
            }
        }
        return;
    }
    let Some(path) = maven_path(&lib.name) else {
        return;
    };
    let url = lib.url.as_deref().map(|base| join_maven_url(base, &path));
    lib.downloads = Some(LibraryDownloads {
        artifact: Some(Artifact {
            path: Some(path),
            sha1: None,
            size: None,
            url,
        }),
        classifiers: None,
    });
}

pub(crate) fn maven_path(coord: &str) -> Option<String> {
    let coord = coord.trim();
    if coord.is_empty() {
        return None;
    }
    let (coord, ext) = match coord.rsplit_once('@') {
        Some((c, e)) if !e.is_empty() => (c, e),
        _ => (coord, "jar"),
    };
    let mut parts = coord.split(':');
    let group = parts.next()?.replace('.', "/");
    let artifact = parts.next()?;
    let version = parts.next()?;
    if group.is_empty() || artifact.is_empty() || version.is_empty() {
        return None;
    }
    let classifier = parts.next();
    let file = match classifier {
        Some(c) if !c.is_empty() => format!("{artifact}-{version}-{c}.{ext}"),
        _ => format!("{artifact}-{version}.{ext}"),
    };
    Some(format!("{group}/{artifact}/{version}/{file}"))
}

fn join_maven_url(base: &str, path: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

fn cmp_forge_version(a: &str, b: &str) -> std::cmp::Ordering {
    version_nums(a).cmp(&version_nums(b)).then_with(|| a.cmp(b))
}

fn version_nums(s: &str) -> Vec<u64> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect()
}

fn read_zip_json<T: DeserializeOwned>(
    zip: &mut zip::ZipArchive<std::fs::File>,
    jar: &Path,
    name: &str,
) -> Result<T, EngineError> {
    let mut entry = zip
        .by_name(name)
        .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|e| EngineError::io(jar, e))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))
}

pub(crate) fn extract_zip_entry(jar: &Path, name: &str, dest: &Path) -> Result<(), EngineError> {
    if dest.is_file() && file_nonempty(dest) {
        return Ok(());
    }
    let file = std::fs::File::open(jar).map_err(|e| EngineError::io(jar, e))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?;
    let needle = name.trim_start_matches('/').replace('\\', "/");
    let mut found = None;
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?;
        let ename = entry.name().replace('\\', "/");
        let trimmed = ename.trim_start_matches('/');
        if trimmed == needle || trimmed == name || ename == name {
            found = Some(i);
            break;
        }
    }
    let Some(index) = found else {
        return Err(EngineError::io(
            jar,
            io::Error::new(io::ErrorKind::NotFound, format!("zip entry {name} missing")),
        ));
    };
    let mut entry = zip
        .by_index(index)
        .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?;
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
        }
    }
    let mut out = std::fs::File::create(dest).map_err(|e| EngineError::io(dest, e))?;
    io::copy(&mut entry, &mut out).map_err(|e| EngineError::io(dest, e))?;
    Ok(())
}

fn file_nonempty(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EngineError;
    use crate::mojang::VersionInfo;
    use std::io::Write;
    use std::path::Path;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn load_version(name: &str) -> VersionInfo {
        let text = std::fs::read_to_string(fixtures_dir().join(name)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn pick_newest_forge_for_mc() {
        let vs = vec![
            "1.20.1-47.1.0".into(),
            "1.20.1-47.2.0".into(),
            "1.19.4-45.0.0".into(),
        ];
        assert_eq!(
            pick_forge_version("1.20.1", &vs, None).unwrap(),
            "1.20.1-47.2.0"
        );
        assert_eq!(
            pick_forge_version("1.20.1", &vs, Some("1.20.1-47.1.0")).unwrap(),
            "1.20.1-47.1.0"
        );
        assert!(pick_forge_version("1.18.2", &vs, None).is_err());
    }

    #[test]
    fn merge_forge_overrides_main_class() {
        let merged = merge_forge(
            load_version("version_1_21.json"),
            load_version("forge_version.json"),
        );
        assert_eq!(
            merged.main_class,
            "cpw.mods.bootstraplauncher.BootstrapLauncher"
        );
    }

    #[test]
    fn substitute_processor_args() {
        let data = std::collections::HashMap::new();
        let out = subst_arg(
            "{MINECRAFT_JAR}",
            &data,
            Path::new("/c.jar"),
            Path::new("/inst.jar"),
        );
        assert_eq!(out, "/c.jar");
    }

    #[test]
    fn parse_maven_versions_reads_version_tags() {
        let xml = r#"<metadata><versioning><versions>
            <version>1.20.1-47.1.0</version>
            <version>1.19.4-45.0.0</version>
        </versions></versioning></metadata>"#;
        assert_eq!(
            parse_maven_versions(xml).unwrap(),
            vec!["1.20.1-47.1.0", "1.19.4-45.0.0"]
        );
    }

    #[test]
    fn installer_url_uses_forge_maven() {
        assert_eq!(
            installer_url("1.20.1-47.2.0"),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.2.0/forge-1.20.1-47.2.0-installer.jar"
        );
    }

    #[test]
    fn merge_appends_forge_library_and_args() {
        let merged = merge_forge(
            load_version("version_1_21.json"),
            load_version("forge_version.json"),
        );
        assert!(
            merged
                .libraries
                .iter()
                .any(|l| l.name == "net.minecraftforge:forge:1.21.1-52.0.0")
        );
        let jvm = &merged.arguments.as_ref().unwrap().jvm;
        assert!(jvm.iter().any(|a| matches!(
            a,
            crate::mojang::LaunchArgument::Value(v) if v == "-DlibraryDirectory=${library_directory}"
        )));
    }

    #[test]
    fn read_installer_reads_zip_entries() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("installer.jar");
        let file = std::fs::File::create(&jar).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("install_profile.json", opts).unwrap();
        zip.write_all(&std::fs::read(fixtures_dir().join("forge_install_profile.json")).unwrap())
            .unwrap();
        zip.start_file("version.json", opts).unwrap();
        zip.write_all(&std::fs::read(fixtures_dir().join("forge_version.json")).unwrap())
            .unwrap();
        zip.finish().unwrap();

        let (profile, version) = read_installer(&jar).unwrap();
        assert_eq!(
            version.main_class,
            "cpw.mods.bootstraplauncher.BootstrapLauncher"
        );
        assert_eq!(profile.processors.len(), 2);
        assert_eq!(profile.installer_path, jar);
    }

    #[tokio::test]
    async fn run_processors_skips_server_only() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let profile = ForgeInstallProfile {
            processors: vec![ForgeProcessor {
                sides: vec!["server".into()],
                jar: "net.minecraftforge:missing:1.0".into(),
                classpath: vec![],
                args: vec![],
            }],
            ..Default::default()
        };
        run_processors(
            Path::new("/nonexistent-java"),
            &profile,
            &paths,
            Path::new("/c.jar"),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn processor_nonzero_exit_is_io() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let jar = paths
            .cache_libraries
            .join("net/minecraftforge/tools/1.0/tools-1.0.jar");
        std::fs::create_dir_all(jar.parent().unwrap()).unwrap();
        let file = std::fs::File::create(&jar).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("META-INF/MANIFEST.MF", opts).unwrap();
        zip.write_all(b"Manifest-Version: 1.0\nMain-Class: example.Proc\n")
            .unwrap();
        zip.finish().unwrap();

        let profile = ForgeInstallProfile {
            processors: vec![ForgeProcessor {
                sides: vec!["client".into()],
                jar: "net.minecraftforge:tools:1.0".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let err = run_processors(
            Path::new("/usr/bin/false"),
            &profile,
            &paths,
            Path::new("/c.jar"),
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        match err {
            EngineError::Io { path, source } => {
                assert_eq!(path, jar);
                assert_eq!(source.kind(), std::io::ErrorKind::Other);
                assert_eq!(source.to_string(), "forge processor exited 1");
            }
            other => panic!("expected Io, got {other:?}"),
        }
    }
}
