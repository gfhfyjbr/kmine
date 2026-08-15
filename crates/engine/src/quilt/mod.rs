use crate::error::EngineError;
use crate::ids::Loader;
use crate::mojang::{Artifact, Library, LibraryDownloads, VersionArguments, VersionInfo};
use serde::Deserialize;

pub const LOADER_INDEX_URL: &str = "https://meta.quiltmc.org/v3/versions/loader";

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLoaderIndex(pub Vec<QuiltLoaderEntry>);

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLoaderEntry {
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
pub struct QuiltProfile {
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<QuiltLibrary>,
    #[serde(default)]
    pub arguments: Option<VersionArguments>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLibrary {
    pub name: String,
    pub url: Option<String>,
}

pub fn pick_loader_version(
    index: &QuiltLoaderIndex,
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
        loader: Loader::Quilt,
        minecraft: String::new(),
    })
}

pub fn profile_url(mc: &str, loader: &str) -> String {
    format!("https://meta.quiltmc.org/v3/versions/loader/{mc}/{loader}/profile/json")
}

pub fn merge_quilt(mut vanilla: VersionInfo, profile: QuiltProfile) -> VersionInfo {
    vanilla.main_class = profile.main_class;
    for lib in profile.libraries {
        vanilla.libraries.push(library_from_quilt(lib));
    }
    if let Some(quilt_args) = profile.arguments {
        match vanilla.arguments.as_mut() {
            Some(existing) => {
                existing.game.extend(quilt_args.game);
                existing.jvm.extend(quilt_args.jvm);
            }
            None => vanilla.arguments = Some(quilt_args),
        }
    }
    vanilla
}

fn library_from_quilt(lib: QuiltLibrary) -> Library {
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

    fn load_loader_index() -> QuiltLoaderIndex {
        let text = std::fs::read_to_string(fixtures_dir().join("quilt_loader.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn load_profile() -> QuiltProfile {
        let text = std::fs::read_to_string(fixtures_dir().join("quilt_profile.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn load_version(name: &str) -> VersionInfo {
        let text = std::fs::read_to_string(fixtures_dir().join(name)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn pick_stable_loader() {
        let idx = load_loader_index();
        assert_eq!(pick_loader_version(&idx, None).unwrap(), "0.27.1");
        assert_eq!(pick_loader_version(&idx, Some("0.26.0")).unwrap(), "0.26.0");
    }

    #[test]
    fn profile_url_uses_meta_quilt_v3() {
        assert_eq!(
            profile_url("1.21.1", "0.27.1"),
            "https://meta.quiltmc.org/v3/versions/loader/1.21.1/0.27.1/profile/json"
        );
    }

    #[test]
    fn merge_replaces_main_class_and_adds_lib() {
        let v = merge_quilt(load_version("version_1_21.json"), load_profile());
        assert_eq!(
            v.main_class,
            "org.quiltmc.loader.impl.launch.knot.KnotClient"
        );
        assert!(v.libraries.iter().any(|l| l.name.contains("quilt-loader")));
    }

    #[test]
    fn pick_empty_index_without_preferred_is_unavailable() {
        let err = pick_loader_version(&QuiltLoaderIndex(Vec::new()), None).unwrap_err();
        assert!(matches!(
            err,
            EngineError::LoaderUnavailable {
                loader: Loader::Quilt,
                ..
            }
        ));
    }
}
