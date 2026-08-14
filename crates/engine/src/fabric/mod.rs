use crate::error::EngineError;
use crate::ids::Loader;
use crate::mojang::{Artifact, Library, LibraryDownloads, VersionInfo};
use serde::Deserialize;

pub const LOADER_INDEX_URL: &str = "https://meta.fabricmc.net/v2/versions/loader";

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLoaderIndex(pub Vec<FabricLoaderEntry>);

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLoaderEntry {
    pub version: String,
    #[serde(default)]
    pub stable: bool,
    #[serde(default)]
    pub separator: Option<String>,
    #[serde(default)]
    pub build: Option<i64>,
    #[serde(default)]
    pub maven: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricProfile {
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<FabricLibrary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLibrary {
    pub name: String,
    pub url: Option<String>,
}

pub fn pick_loader_version(
    index: &FabricLoaderIndex,
    preferred: Option<&str>,
) -> Result<String, EngineError> {
    if let Some(preferred) = preferred {
        return Ok(preferred.to_string());
    }
    if let Some(entry) = index.0.iter().find(|e| e.stable) {
        return Ok(entry.version.clone());
    }
    if let Some(entry) = index.0.first() {
        return Ok(entry.version.clone());
    }
    Err(EngineError::LoaderUnavailable {
        loader: Loader::Fabric,
        minecraft: String::new(),
    })
}

pub fn profile_url(mc: &str, loader: &str) -> String {
    format!("https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/profile/json")
}

pub fn merge_fabric(mut vanilla: VersionInfo, profile: FabricProfile) -> VersionInfo {
    vanilla.main_class = profile.main_class;
    for lib in profile.libraries {
        vanilla.libraries.push(library_from_fabric(lib));
    }
    vanilla
}

fn library_from_fabric(lib: FabricLibrary) -> Library {
    let path = maven_path(&lib.name);
    let url = match (lib.url.as_deref(), path.as_deref()) {
        (Some(base), Some(path)) => Some(join_maven_url(base, path)),
        _ => None,
    };
    Library {
        name: lib.name,
        downloads: path.map(|path| LibraryDownloads {
            artifact: Some(Artifact {
                path: Some(path),
                sha1: None,
                size: None,
                url,
            }),
            classifiers: None,
        }),
        rules: Vec::new(),
        natives: None,
        extract: None,
        url: lib.url,
    }
}

fn maven_path(name: &str) -> Option<String> {
    let mut parts = name.split(':');
    let group = parts.next()?.replace('.', "/");
    let artifact = parts.next()?;
    let version = parts.next()?;
    if group.is_empty() || artifact.is_empty() || version.is_empty() {
        return None;
    }
    match parts.next() {
        Some(classifier) if !classifier.is_empty() => Some(format!(
            "{group}/{artifact}/{version}/{artifact}-{version}-{classifier}.jar"
        )),
        _ => Some(format!(
            "{group}/{artifact}/{version}/{artifact}-{version}.jar"
        )),
    }
}

fn join_maven_url(base: &str, path: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mojang::VersionInfo;

    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn load_loader_index() -> FabricLoaderIndex {
        let text = std::fs::read_to_string(fixtures_dir().join("fabric_loader.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn load_profile() -> FabricProfile {
        let text = std::fs::read_to_string(fixtures_dir().join("fabric_profile.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn load_version(name: &str) -> VersionInfo {
        let text = std::fs::read_to_string(fixtures_dir().join(name)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn pick_stable_loader() {
        let idx = load_loader_index();
        assert_eq!(pick_loader_version(&idx, None).unwrap(), "0.16.0");
        assert_eq!(pick_loader_version(&idx, Some("0.15.0")).unwrap(), "0.15.0");
    }

    #[test]
    fn merge_replaces_main_class_and_adds_lib() {
        let v = merge_fabric(load_version("version_1_21.json"), load_profile());
        assert_eq!(
            v.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert!(v.libraries.iter().any(|l| l.name.contains("fabric-loader")));
    }

    #[test]
    fn profile_url_uses_meta_fabric() {
        assert_eq!(
            profile_url("1.21.1", "0.16.0"),
            "https://meta.fabricmc.net/v2/versions/loader/1.21.1/0.16.0/profile/json"
        );
    }

    #[test]
    fn pick_empty_index_without_preferred_is_unavailable() {
        let err = pick_loader_version(&FabricLoaderIndex(Vec::new()), None).unwrap_err();
        assert!(matches!(
            err,
            EngineError::LoaderUnavailable {
                loader: Loader::Fabric,
                ..
            }
        ));
    }

    #[test]
    fn merge_appends_maven_path_so_libraries_download() {
        let v = merge_fabric(load_version("version_1_21.json"), load_profile());
        let arts = crate::mojang::select_libraries(&v);
        assert!(arts.iter().any(|a| {
            a.path == "net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar"
                && a.url
                    == "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar"
        }));
    }
}
