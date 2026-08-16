//! NeoForge installer prepare (version pick + download). Processors reuse forge.

use crate::error::EngineError;
use crate::forge::{self, ForgeInstallProfile};
use crate::http::HttpFiles;
use crate::ids::Loader;
use crate::mojang::VersionInfo;
use crate::paths::LauncherPaths;
use crate::types::{PrepareMode, ProgressSink};
use tokio_util::sync::CancellationToken;

pub const MAVEN_METADATA_URL: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

pub fn is_legacy_forge_artifact(ver: &str) -> bool {
    ver.split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .is_some_and(|n| n >= 40)
}

pub fn minecraft_from_neoforge(ver: &str) -> Option<String> {
    if is_legacy_forge_artifact(ver) {
        return None;
    }
    let mut parts = ver.split('.');
    let major = parts.next()?;
    let minor = parts.next()?;
    Some(format!("1.{major}.{minor}"))
}

pub fn normalize_minecraft(mc: &str) -> String {
    let dots = mc.bytes().filter(|&b| b == b'.').count();
    if dots == 1 {
        format!("{mc}.0")
    } else {
        mc.to_string()
    }
}

pub fn pick_neoforge_version(
    mc: &str,
    versions: &[String],
    preferred: Option<&str>,
) -> Result<String, EngineError> {
    if let Some(preferred) = preferred {
        return Ok(preferred.to_string());
    }
    let want = normalize_minecraft(mc);
    versions
        .iter()
        .filter(|v| minecraft_from_neoforge(v).as_deref() == Some(want.as_str()))
        .max_by(|a, b| forge::cmp_forge_version(a, b))
        .cloned()
        .ok_or_else(|| EngineError::LoaderUnavailable {
            loader: Loader::NeoForge,
            minecraft: mc.to_string(),
        })
}

pub fn installer_url(ver: &str) -> String {
    if is_legacy_forge_artifact(ver) {
        format!(
            "https://maven.neoforged.net/releases/net/neoforged/forge/{ver}/forge-{ver}-installer.jar"
        )
    } else {
        format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{ver}/neoforge-{ver}-installer.jar"
        )
    }
}

pub async fn prepare_neoforge(
    http: &HttpFiles,
    paths: &LauncherPaths,
    mc: &str,
    preferred: Option<&str>,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
    mode: PrepareMode,
) -> Result<(ForgeInstallProfile, VersionInfo), EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    progress.set("NeoForge installer", 0, 2);
    let meta_path = paths.cache_meta.join("neoforge-maven-metadata.xml");
    let xml_bytes = http
        .load_meta_bytes(MAVEN_METADATA_URL, &meta_path, mode, cancel)
        .await?;
    let xml = String::from_utf8(xml_bytes)
        .map_err(|e| EngineError::io(&meta_path, std::io::Error::other(e.to_string())))?;
    let versions = forge::parse_maven_versions(&xml)?;
    let ver = match pick_neoforge_version(mc, &versions, preferred) {
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
    progress.set("NeoForge installer", 1, 2);
    let url = installer_url(&ver);
    let artifact = if is_legacy_forge_artifact(&ver) {
        "forge"
    } else {
        "neoforge"
    };
    let installer_path = paths.cache_libraries.join(format!(
        "net/neoforged/{artifact}/{ver}/{artifact}-{ver}-installer.jar"
    ));
    let sha1_path = installer_path.with_extension("jar.sha1");
    let sha1 = forge::fetch_installer_sha1(http, &url, &sha1_path, cancel, mode).await?;
    match http
        .download_sha1(&url, &installer_path, sha1.as_deref(), None, cancel, mode)
        .await
    {
        Err(EngineError::Http { status: 404, .. }) => {
            return Err(EngineError::LoaderUnavailable {
                loader: Loader::NeoForge,
                minecraft: mc.to_string(),
            });
        }
        other => other?,
    }

    let (profile, version) = forge::read_installer(&installer_path)?;
    progress.set("NeoForge installer", 2, 2);
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    forge::fetch_installer_libraries(http, paths, &profile, progress, cancel, mode).await?;
    Ok((profile, version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EngineError;
    use crate::ids::Loader;

    #[test]
    fn installer_url_modern() {
        assert_eq!(
            installer_url("21.1.66"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.66/neoforge-21.1.66-installer.jar"
        );
    }

    #[test]
    fn installer_url_legacy_47() {
        assert_eq!(
            installer_url("47.1.106"),
            "https://maven.neoforged.net/releases/net/neoforged/forge/47.1.106/forge-47.1.106-installer.jar"
        );
    }

    #[test]
    fn pick_filters_by_minecraft() {
        let versions = ["21.1.1", "21.0.1", "20.4.1"].map(String::from);
        assert_eq!(
            pick_neoforge_version("1.21.1", &versions, None).unwrap(),
            "21.1.1"
        );
    }

    #[test]
    fn pick_treats_1_21_as_1_21_0() {
        let versions = ["21.0.3", "21.1.1"].map(String::from);
        assert_eq!(
            pick_neoforge_version("1.21", &versions, None).unwrap(),
            "21.0.3"
        );
    }

    #[test]
    fn pick_preferred_wins_even_if_legacy() {
        let versions = ["21.1.1"].map(String::from);
        assert_eq!(
            pick_neoforge_version("1.21.1", &versions, Some("47.1.106")).unwrap(),
            "47.1.106"
        );
    }

    #[test]
    fn pick_empty_is_unavailable() {
        let err = pick_neoforge_version("1.21.1", &[], None).unwrap_err();
        assert!(matches!(
            err,
            EngineError::LoaderUnavailable {
                loader: Loader::NeoForge,
                ..
            }
        ));
    }
}
