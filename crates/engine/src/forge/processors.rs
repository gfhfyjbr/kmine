use super::{ForgeInstallProfile, ForgeProcessor, extract_zip_entry, maven_path};
use crate::error::EngineError;
use crate::mojang::join_classpath;
use crate::paths::LauncherPaths;
use crate::types::PrepareMode;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio_util::sync::CancellationToken;

pub fn processor_stamp_path(paths: &LauncherPaths, installer: &Path) -> PathBuf {
    let stem = installer
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("installer");
    paths
        .cache_meta
        .join("forge-processors")
        .join(format!("{stem}.ok"))
}

pub fn installer_sha1(installer: &Path) -> Result<String, EngineError> {
    let mut file = std::fs::File::open(installer).map_err(|e| EngineError::io(installer, e))?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| EngineError::io(installer, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn subst_arg(
    arg: &str,
    data: &HashMap<String, String>,
    vanilla_client: &Path,
    installer: &Path,
) -> String {
    replace_braces(arg, |key| match key {
        "MINECRAFT_JAR" => Some(vanilla_client.to_string_lossy().into_owned()),
        "INSTALLER" => Some(installer.to_string_lossy().into_owned()),
        "SIDE" => Some("client".into()),
        other => data.get(other).cloned(),
    })
}

pub async fn run_processors(
    java: &Path,
    profile: &ForgeInstallProfile,
    paths: &LauncherPaths,
    vanilla_client: &Path,
    cancel: &CancellationToken,
    mode: PrepareMode,
) -> Result<(), EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    let stamp = processor_stamp_path(paths, profile.installer_path.as_path());
    let current = installer_sha1(profile.installer_path.as_path())?;
    if mode == PrepareMode::Warm {
        if let Ok(body) = std::fs::read_to_string(&stamp) {
            if body.trim() == current {
                return Ok(());
            }
        }
    } else {
        match std::fs::remove_file(&stamp) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(EngineError::io(&stamp, err)),
        }
    }
    let extract_dir = processor_extract_dir(paths, profile.installer_path.as_path());
    std::fs::create_dir_all(&extract_dir).map_err(|e| EngineError::io(&extract_dir, e))?;
    let mut data = resolve_data(profile, paths, &extract_dir)?;
    let libraries = paths.cache_libraries.to_string_lossy().into_owned();
    data.insert("ROOT".into(), libraries.clone());
    data.insert("LIBRARY_DIR".into(), libraries);
    if let Some(mc) = &profile.minecraft {
        data.insert("MINECRAFT_VERSION".into(), mc.clone());
    }
    let installer = profile.installer_path.as_path();
    for proc in &profile.processors {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        if is_server_only(&proc.sides) {
            continue;
        }
        run_one(java, proc, paths, &data, vanilla_client, installer, cancel).await?;
    }
    if let Some(parent) = stamp.parent() {
        std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
    }
    std::fs::write(&stamp, current.as_bytes()).map_err(|e| EngineError::io(&stamp, e))?;
    Ok(())
}

pub(crate) fn processor_extract_dir(paths: &LauncherPaths, installer: &Path) -> std::path::PathBuf {
    let stem = installer
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("installer");
    paths.cache_meta.join("forge-extract").join(stem)
}

fn is_server_only(sides: &[String]) -> bool {
    !sides.is_empty() && sides.iter().all(|side| side == "server")
}

fn resolve_data(
    profile: &ForgeInstallProfile,
    paths: &LauncherPaths,
    extract_dir: &Path,
) -> Result<HashMap<String, String>, EngineError> {
    let mut out = HashMap::new();
    for (key, file) in &profile.data {
        out.insert(
            key.clone(),
            resolve_data_value(&file.client, paths, extract_dir, &profile.installer_path)?,
        );
    }
    Ok(out)
}

fn resolve_data_value(
    value: &str,
    paths: &LauncherPaths,
    extract_dir: &Path,
    installer: &Path,
) -> Result<String, EngineError> {
    if value.starts_with('[') && value.ends_with(']') && value.len() >= 2 {
        let coord = &value[1..value.len() - 1];
        let rel = maven_path(coord).ok_or_else(|| {
            EngineError::io(
                paths.cache_libraries.clone(),
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("bad maven coord {coord}"),
                ),
            )
        })?;
        return Ok(paths
            .cache_libraries
            .join(rel)
            .to_string_lossy()
            .into_owned());
    }
    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return Ok(value[1..value.len() - 1].to_string());
    }
    let rel = value.trim_start_matches('/');
    let dest = extract_dir.join(rel);
    if !installer.as_os_str().is_empty() {
        extract_zip_entry(installer, rel, &dest)?;
    }
    Ok(dest.to_string_lossy().into_owned())
}

async fn run_one(
    java: &Path,
    proc: &ForgeProcessor,
    paths: &LauncherPaths,
    data: &HashMap<String, String>,
    vanilla_client: &Path,
    installer: &Path,
    cancel: &CancellationToken,
) -> Result<(), EngineError> {
    let rel = maven_path(&proc.jar).ok_or_else(|| {
        EngineError::io(
            paths.cache_libraries.clone(),
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("bad processor jar {}", proc.jar),
            ),
        )
    })?;
    let processor_jar = paths.cache_libraries.join(rel);
    if !processor_jar.is_file() {
        return Err(EngineError::io(
            &processor_jar,
            io::Error::new(io::ErrorKind::NotFound, "processor jar missing"),
        ));
    }
    let main = main_class_of(&processor_jar)?;
    let mut classpath = vec![processor_jar.clone()];
    for coord in &proc.classpath {
        let Some(rel) = maven_path(coord) else {
            continue;
        };
        classpath.push(paths.cache_libraries.join(rel));
    }
    let args: Vec<String> = proc
        .args
        .iter()
        .map(|arg| subst_processor_arg(arg, data, vanilla_client, installer, paths))
        .collect();

    let mut cmd = tokio::process::Command::new(java);
    cmd.arg("-cp")
        .arg(join_classpath(&classpath))
        .arg(&main)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| EngineError::io(java, e))?;
    let mut stderr_pipe = child.stderr.take();
    let status = tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.start_kill();
            return Err(EngineError::Cancelled);
        }
        status = child.wait() => status.map_err(|e| EngineError::io(java, e))?,
    };
    if !status.success() {
        let code = status.code().unwrap_or(-1);
        let mut buf = Vec::new();
        if let Some(mut pipe) = stderr_pipe.take() {
            use tokio::io::AsyncReadExt;
            let _ = pipe.read_to_end(&mut buf).await;
        }
        let detail = String::from_utf8_lossy(&buf);
        let detail = detail.trim();
        let message = if detail.is_empty() {
            format!("forge processor exited {code}")
        } else {
            let clipped: String = detail.chars().take(800).collect();
            format!("forge processor exited {code}: {clipped}")
        };
        return Err(EngineError::Io {
            path: processor_jar,
            source: io::Error::new(io::ErrorKind::Other, message),
        });
    }
    Ok(())
}

fn subst_processor_arg(
    arg: &str,
    data: &HashMap<String, String>,
    vanilla_client: &Path,
    installer: &Path,
    paths: &LauncherPaths,
) -> String {
    if arg.starts_with('[') && arg.ends_with(']') && arg.len() >= 2 {
        if let Some(rel) = maven_path(&arg[1..arg.len() - 1]) {
            return paths
                .cache_libraries
                .join(rel)
                .to_string_lossy()
                .into_owned();
        }
    }
    subst_arg(arg, data, vanilla_client, installer)
}

fn main_class_of(jar: &Path) -> Result<String, EngineError> {
    let file = std::fs::File::open(jar).map_err(|e| EngineError::io(jar, e))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?;
    let mut text = String::new();
    let mut index = None;
    for i in 0..zip.len() {
        let name = zip
            .by_index(i)
            .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?
            .name()
            .replace('\\', "/");
        if name.eq_ignore_ascii_case("META-INF/MANIFEST.MF") {
            index = Some(i);
            break;
        }
    }
    let Some(index) = index else {
        return Err(EngineError::io(
            jar,
            io::Error::new(io::ErrorKind::NotFound, "processor jar missing manifest"),
        ));
    };
    zip.by_index(index)
        .map_err(|e| EngineError::io(jar, io::Error::other(e.to_string())))?
        .read_to_string(&mut text)
        .map_err(|e| EngineError::io(jar, e))?;
    let mut unfolded = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(' ') {
            unfolded.push_str(rest);
        } else {
            unfolded.push('\n');
            unfolded.push_str(line);
        }
    }
    for line in unfolded.lines() {
        if let Some(rest) = line.strip_prefix("Main-Class:") {
            let name = rest.trim();
            if !name.is_empty() {
                return Ok(name.to_string());
            }
        }
    }
    Err(EngineError::io(
        jar,
        io::Error::new(
            io::ErrorKind::InvalidData,
            "processor jar missing Main-Class",
        ),
    ))
}

fn replace_braces(input: &str, mut lookup: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match lookup(key) {
                    Some(val) => out.push_str(&val),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::{ForgeInstallProfile, ForgeProcessor};
    use crate::types::PrepareMode;
    use std::path::PathBuf;

    #[test]
    fn processor_stamp_path_uses_installer_stem() {
        let paths = LauncherPaths::new(PathBuf::from("/data/kmine"));
        let stamp = processor_stamp_path(
            &paths,
            Path::new("/cache/forge-1.21.1-52.0.0-installer.jar"),
        );
        assert!(
            stamp.ends_with("cache/meta/forge-processors/forge-1.21.1-52.0.0-installer.ok")
                || stamp
                    .ends_with("cache\\meta\\forge-processors\\forge-1.21.1-52.0.0-installer.ok")
        );
    }

    #[tokio::test]
    async fn run_processors_warm_skips_when_stamp_matches() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let installer = root.path().join("inst.jar");
        std::fs::write(&installer, b"installer-bytes").unwrap();
        let sha = installer_sha1(&installer).unwrap();
        let stamp = processor_stamp_path(&paths, &installer);
        std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
        std::fs::write(&stamp, sha).unwrap();

        let profile = ForgeInstallProfile {
            processors: vec![ForgeProcessor {
                sides: vec!["client".into()],
                jar: "net.minecraftforge:installertools:1.0.0".into(),
                classpath: vec![],
                args: vec![],
                ..Default::default()
            }],
            installer_path: installer.clone(),
            ..Default::default()
        };
        // java path can be fake: skip must happen before spawn
        run_processors(
            Path::new("/no/java"),
            &profile,
            &paths,
            Path::new("/no/client.jar"),
            &CancellationToken::new(),
            PrepareMode::Warm,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn run_processors_verify_deletes_stamp_and_runs() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let installer = root.path().join("inst.jar");
        std::fs::write(&installer, b"installer-bytes").unwrap();
        let sha = installer_sha1(&installer).unwrap();
        let stamp = processor_stamp_path(&paths, &installer);
        std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
        std::fs::write(&stamp, &sha).unwrap();

        let profile = ForgeInstallProfile {
            processors: vec![ForgeProcessor {
                sides: vec!["client".into()],
                jar: "net.minecraftforge:installertools:1.0.0".into(),
                classpath: vec![],
                args: vec![],
                ..Default::default()
            }],
            installer_path: installer.clone(),
            ..Default::default()
        };
        let err = run_processors(
            Path::new("/no/java"),
            &profile,
            &paths,
            Path::new("/no/client.jar"),
            &CancellationToken::new(),
            PrepareMode::Verify,
        )
        .await
        .unwrap_err();
        let _ = err;
        assert!(!stamp.exists(), "verify must delete stamp before run");
    }

    #[tokio::test]
    async fn warm_does_not_skip_processors_after_verify_deleted_stamp() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let installer = root.path().join("inst.jar");
        std::fs::write(&installer, b"installer-bytes").unwrap();
        let sha = installer_sha1(&installer).unwrap();
        let stamp = processor_stamp_path(&paths, &installer);
        std::fs::create_dir_all(stamp.parent().unwrap()).unwrap();
        std::fs::write(&stamp, &sha).unwrap();

        let profile = ForgeInstallProfile {
            processors: vec![ForgeProcessor {
                sides: vec!["client".into()],
                jar: "net.minecraftforge:installertools:1.0.0".into(),
                classpath: vec![],
                args: vec![],
                ..Default::default()
            }],
            installer_path: installer.clone(),
            ..Default::default()
        };
        let verify_err = run_processors(
            Path::new("/no/java"),
            &profile,
            &paths,
            Path::new("/no/client.jar"),
            &CancellationToken::new(),
            PrepareMode::Verify,
        )
        .await
        .unwrap_err();
        let _ = verify_err;
        assert!(!stamp.exists());

        // stamp absent → Warm must not return Ok before run_one
        let warm_err = run_processors(
            Path::new("/no/java"),
            &profile,
            &paths,
            Path::new("/no/client.jar"),
            &CancellationToken::new(),
            PrepareMode::Warm,
        )
        .await
        .unwrap_err();
        let _ = warm_err;
        assert!(!stamp.exists(), "failed Warm must not write stamp");
    }
}
