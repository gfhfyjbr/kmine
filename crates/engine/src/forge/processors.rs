use super::{ForgeInstallProfile, ForgeProcessor, extract_zip_entry, maven_path};
use crate::error::EngineError;
use crate::mojang::join_classpath;
use crate::paths::LauncherPaths;
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::Path;
use std::process::Stdio;
use tokio_util::sync::CancellationToken;

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
) -> Result<(), EngineError> {
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
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
