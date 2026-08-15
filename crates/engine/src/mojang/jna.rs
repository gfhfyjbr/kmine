use super::{ArgValue, Artifact, LaunchArgument, Library, VersionInfo};

/// JNA 5.12.1 and older abort on macOS 12+ when `dlerror()` is longer than
/// 1024 bytes (`LOAD_ERROR` in `dispatch.c`). 1.17–1.20.1 ship those
/// versions; 5.13.0 fixed the assert. Pin the Mojang 1.21.1 artifacts.
const JNA_PIN: &str = "5.14.0";
const JNA_MIN: (u32, u32, u32) = (5, 13, 0);

const JNA_SHA1: &str = "67bf3eaea4f0718cb376a181a629e5f88fa1c9dd";
const JNA_SIZE: u64 = 1_878_533;
const JNA_PLATFORM_SHA1: &str = "28934d48aed814f11e4c584da55c49fa7032b31b";
const JNA_PLATFORM_SIZE: u64 = 1_369_287;

pub fn apply_legacy_jna_workaround(version: &mut VersionInfo) {
    if cfg!(target_os = "macos") {
        upgrade_legacy_jna(version);
    }
}

pub fn upgrade_legacy_jna(version: &mut VersionInfo) {
    let mut changed = false;
    for lib in &mut version.libraries {
        if rewrite_jna_library(lib) {
            changed = true;
        }
    }
    if changed {
        if let Some(args) = version.arguments.as_mut() {
            for arg in &mut args.jvm {
                rewrite_launch_arg(arg);
            }
        }
    }
}

fn rewrite_jna_library(lib: &mut Library) -> bool {
    let Some((artifact, _old)) = jna_artifact_to_pin(&lib.name) else {
        return false;
    };
    lib.name = format!("net.java.dev.jna:{artifact}:{JNA_PIN}");
    let path = format!("net/java/dev/jna/{artifact}/{JNA_PIN}/{artifact}-{JNA_PIN}.jar");
    let url = format!("https://libraries.minecraft.net/{path}");
    let (sha1, size) = if artifact == "jna-platform" {
        (JNA_PLATFORM_SHA1, JNA_PLATFORM_SIZE)
    } else {
        (JNA_SHA1, JNA_SIZE)
    };
    if let Some(downloads) = lib.downloads.as_mut() {
        match downloads.artifact.as_mut() {
            Some(art) => apply_artifact(art, &path, &url, sha1, size),
            None => {
                downloads.artifact = Some(pinned_artifact(&path, &url, sha1, size));
            }
        }
    } else {
        lib.downloads = Some(super::LibraryDownloads {
            artifact: Some(pinned_artifact(&path, &url, sha1, size)),
            classifiers: None,
        });
    }
    lib.url = Some(url);
    true
}

fn apply_artifact(art: &mut Artifact, path: &str, url: &str, sha1: &str, size: u64) {
    art.path = Some(path.to_string());
    art.url = Some(url.to_string());
    art.sha1 = Some(sha1.to_string());
    art.size = Some(size);
}

fn pinned_artifact(path: &str, url: &str, sha1: &str, size: u64) -> Artifact {
    Artifact {
        path: Some(path.to_string()),
        url: Some(url.to_string()),
        sha1: Some(sha1.to_string()),
        size: Some(size),
    }
}

fn jna_artifact_to_pin(name: &str) -> Option<(&'static str, &str)> {
    let mut parts = name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    if parts.next().is_some() || group != "net.java.dev.jna" {
        return None;
    }
    let pin_artifact = match artifact {
        "jna" => "jna",
        "jna-platform" => "jna-platform",
        _ => return None,
    };
    if !is_legacy_jna(version) {
        return None;
    }
    Some((pin_artifact, version))
}

fn is_legacy_jna(version: &str) -> bool {
    match parse_semver(version) {
        Some(parsed) => parsed < JNA_MIN,
        None => false,
    }
}

fn parse_semver(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch_raw = parts.next().unwrap_or("0");
    let patch_digits: String = patch_raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let patch = if patch_digits.is_empty() {
        0
    } else {
        patch_digits.parse().ok()?
    };
    Some((major, minor, patch))
}

fn rewrite_launch_arg(arg: &mut LaunchArgument) {
    match arg {
        LaunchArgument::Value(value) => rewrite_merge_modules_string(value),
        LaunchArgument::Ruled { value, .. } => match value {
            ArgValue::One(s) => rewrite_merge_modules_string(s),
            ArgValue::Many(many) => {
                for s in many {
                    rewrite_merge_modules_string(s);
                }
            }
        },
    }
}

fn rewrite_merge_modules_string(value: &mut String) {
    if !value.contains("mergeModules") && !value.contains("jna-") {
        return;
    }
    *value = replace_legacy_jna_jars(value);
}

fn replace_legacy_jna_jars(input: &str) -> String {
    let with_platform =
        replace_prefixed_legacy_jar(input, "jna-platform-", "jna-platform-5.14.0.jar");
    replace_prefixed_legacy_jar(&with_platform, "jna-", "jna-5.14.0.jar")
}

fn replace_prefixed_legacy_jar(input: &str, prefix: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(prefix) {
        if prefix == "jna-" && rest[idx..].starts_with("jna-platform-") {
            let keep_end = idx + prefix.len();
            out.push_str(&rest[..keep_end]);
            rest = &rest[keep_end..];
            continue;
        }
        out.push_str(&rest[..idx]);
        let after = &rest[idx + prefix.len()..];
        if let Some(end) = after.find(".jar") {
            let ver = &after[..end];
            if is_legacy_jna(ver) {
                out.push_str(replacement);
                rest = &after[end + 4..];
                continue;
            }
        }
        out.push_str(prefix);
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::{JNA_PIN, apply_legacy_jna_workaround, upgrade_legacy_jna};
    use crate::mojang::VersionInfo;

    fn version_json(jna: &str, platform: &str) -> VersionInfo {
        let text = format!(
            r#"{{
                "id": "1.20.1",
                "mainClass": "n.m.Main",
                "arguments": {{
                    "game": [],
                    "jvm": [
                        "-DmergeModules=jna-{jna}.jar,jna-platform-{platform}.jar",
                        "-DlibraryDirectory=${{library_directory}}"
                    ]
                }},
                "libraries": [
                    {{
                        "name": "net.java.dev.jna:jna:{jna}",
                        "downloads": {{
                            "artifact": {{
                                "path": "net/java/dev/jna/jna/{jna}/jna-{jna}.jar",
                                "sha1": "oldjna",
                                "size": 1,
                                "url": "https://libraries.minecraft.net/net/java/dev/jna/jna/{jna}/jna-{jna}.jar"
                            }}
                        }}
                    }},
                    {{
                        "name": "net.java.dev.jna:jna-platform:{platform}",
                        "downloads": {{
                            "artifact": {{
                                "path": "net/java/dev/jna/jna-platform/{platform}/jna-platform-{platform}.jar",
                                "sha1": "oldplat",
                                "size": 2,
                                "url": "https://libraries.minecraft.net/net/java/dev/jna/jna-platform/{platform}/jna-platform-{platform}.jar"
                            }}
                        }}
                    }},
                    {{
                        "name": "com.github.oshi:oshi-core:6.2.2",
                        "downloads": {{
                            "artifact": {{
                                "path": "com/github/oshi/oshi-core/6.2.2/oshi-core-6.2.2.jar",
                                "sha1": "oshi",
                                "size": 3,
                                "url": "https://libraries.minecraft.net/oshi.jar"
                            }}
                        }}
                    }}
                ]
            }}"#,
            jna = jna,
            platform = platform,
        );
        serde_json::from_str(&text).unwrap()
    }

    fn jna_names(version: &VersionInfo) -> Vec<&str> {
        version
            .libraries
            .iter()
            .filter(|lib| lib.name.starts_with("net.java.dev.jna:"))
            .map(|lib| lib.name.as_str())
            .collect()
    }

    fn merge_modules(version: &VersionInfo) -> String {
        version
            .arguments
            .as_ref()
            .unwrap()
            .jvm
            .iter()
            .find_map(|arg| match arg {
                crate::mojang::LaunchArgument::Value(v) if v.starts_with("-DmergeModules=") => {
                    Some(v.clone())
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn upgrades_1_20_1_jna_and_merge_modules() {
        let mut version = version_json("5.12.1", "5.12.1");
        upgrade_legacy_jna(&mut version);
        assert_eq!(
            jna_names(&version),
            [
                format!("net.java.dev.jna:jna:{JNA_PIN}"),
                format!("net.java.dev.jna:jna-platform:{JNA_PIN}")
            ]
        );
        let jna = version
            .libraries
            .iter()
            .find(|lib| lib.name == format!("net.java.dev.jna:jna:{JNA_PIN}"))
            .unwrap();
        let art = jna.downloads.as_ref().unwrap().artifact.as_ref().unwrap();
        assert_eq!(
            art.path.as_deref(),
            Some("net/java/dev/jna/jna/5.14.0/jna-5.14.0.jar")
        );
        assert_eq!(art.sha1.as_deref(), Some(super::JNA_SHA1));
        assert_eq!(art.size, Some(super::JNA_SIZE));
        assert_eq!(
            art.url.as_deref(),
            Some("https://libraries.minecraft.net/net/java/dev/jna/jna/5.14.0/jna-5.14.0.jar")
        );
        assert_eq!(
            merge_modules(&version),
            "-DmergeModules=jna-5.14.0.jar,jna-platform-5.14.0.jar"
        );
        assert!(
            version
                .libraries
                .iter()
                .any(|lib| lib.name == "com.github.oshi:oshi-core:6.2.2")
        );
    }

    #[test]
    fn upgrades_forge_hardcoded_5_10_merge_modules() {
        let mut version = version_json("5.12.1", "5.12.1");
        if let Some(args) = version.arguments.as_mut() {
            args.jvm[0] = crate::mojang::LaunchArgument::Value(
                "-DmergeModules=jna-5.10.0.jar,jna-platform-5.10.0.jar".into(),
            );
        }
        upgrade_legacy_jna(&mut version);
        assert_eq!(
            merge_modules(&version),
            "-DmergeModules=jna-5.14.0.jar,jna-platform-5.14.0.jar"
        );
    }

    #[test]
    fn leaves_modern_jna_alone() {
        let mut version = version_json("5.14.0", "5.14.0");
        upgrade_legacy_jna(&mut version);
        assert_eq!(jna_names(&version)[0], "net.java.dev.jna:jna:5.14.0");
        assert_eq!(
            version.libraries[0]
                .downloads
                .as_ref()
                .unwrap()
                .artifact
                .as_ref()
                .unwrap()
                .sha1
                .as_deref(),
            Some("oldjna")
        );
    }

    #[test]
    fn leaves_5_13_alone() {
        let mut version = version_json("5.13.0", "5.13.0");
        upgrade_legacy_jna(&mut version);
        assert_eq!(jna_names(&version)[0], "net.java.dev.jna:jna:5.13.0");
    }

    #[test]
    fn apply_workaround_only_on_macos() {
        let mut version = version_json("5.12.1", "5.12.1");
        apply_legacy_jna_workaround(&mut version);
        let name = jna_names(&version)[0];
        if cfg!(target_os = "macos") {
            assert_eq!(name, "net.java.dev.jna:jna:5.14.0");
        } else {
            assert_eq!(name, "net.java.dev.jna:jna:5.12.1");
        }
    }
}
