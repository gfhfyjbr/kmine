//! OS sandbox around the game process.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::error::EngineError;
use crate::paths::LauncherPaths;
use crate::types::{LaunchPlan, SandboxSpec, SandboxStatus};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus};

pub fn sandbox_status() -> SandboxStatus {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        SandboxStatus::Available
    }
    #[cfg(target_os = "linux")]
    {
        if bwrap_on_path() {
            SandboxStatus::Available
        } else {
            SandboxStatus::Unavailable {
                reason: "bwrap not found on PATH".into(),
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        SandboxStatus::Unavailable {
            reason: "sandbox not implemented on this OS".into(),
        }
    }
}

pub fn fill_spec(plan: &LaunchPlan, paths: &LauncherPaths) -> SandboxSpec {
    let allow_write = {
        #[cfg(target_os = "linux")]
        {
            vec![
                plan.cwd.clone(),
                plan.natives_dir.clone(),
                linux_xdg_dir(paths, plan),
            ]
        }
        #[cfg(not(target_os = "linux"))]
        {
            vec![plan.cwd.clone(), plan.natives_dir.clone()]
        }
    };

    let mut allow_read = Vec::new();
    allow_read.push(java_home(&plan.java));
    if let Some(parent) = plan.java.parent() {
        push_unique(&mut allow_read, parent.to_path_buf());
    }
    push_unique(&mut allow_read, paths.cache_libraries.clone());
    push_unique(&mut allow_read, paths.cache_assets_objects.clone());
    push_unique(&mut allow_read, paths.cache_assets_indexes.clone());
    push_unique(&mut allow_read, paths.cache_assets_virtual.clone());
    if let Some(assets) = paths.cache_assets_objects.parent() {
        push_unique(&mut allow_read, assets.to_path_buf());
    }
    push_unique(&mut allow_read, paths.cache_runtime.clone());
    push_unique(&mut allow_read, plan.java.clone());

    SandboxSpec {
        enabled: plan.sandbox.enabled,
        allow_read,
        allow_write,
        network: true,
    }
}

pub(crate) fn child_pid(child: &Child) -> u32 {
    #[cfg(windows)]
    {
        windows::jailed_pid(child).unwrap_or_else(|| child.id())
    }
    #[cfg(not(windows))]
    {
        child.id()
    }
}

pub(crate) fn wait_child(child: Child) -> io::Result<ExitStatus> {
    #[cfg(windows)]
    {
        windows::wait_jailed(child)
    }
    #[cfg(not(windows))]
    {
        let mut child = child;
        child.wait()
    }
}

pub fn spawn_sandboxed(plan: &LaunchPlan) -> Result<std::process::Child, EngineError> {
    ensure_write_dirs(plan)?;
    #[cfg(target_os = "macos")]
    {
        macos::spawn(plan)
    }
    #[cfg(target_os = "linux")]
    {
        linux::spawn(plan)
    }
    #[cfg(target_os = "windows")]
    {
        windows::spawn(plan)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(EngineError::SandboxUnavailable {
            reason: "sandbox not implemented on this OS".into(),
        })
    }
}

pub(crate) fn apply_plan_stdio(cmd: &mut std::process::Command, plan: &LaunchPlan) {
    cmd.args(&plan.jvm_args)
        .arg(&plan.main_class)
        .args(&plan.game_args)
        .current_dir(&plan.cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, value) in &plan.env {
        cmd.env(key, value);
    }
    cmd.env("TMPDIR", &plan.cwd);
    cmd.env("TEMP", &plan.cwd);
    cmd.env("TMP", &plan.cwd);
}

pub(crate) fn java_home(java: &Path) -> PathBuf {
    let mut home = java.to_path_buf();
    let name = home.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "java" || name == "java.exe" {
        home.pop();
        if home.file_name().and_then(|n| n.to_str()) == Some("bin") {
            home.pop();
        }
    }
    home
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn instance_slug(plan: &LaunchPlan) -> String {
    let cwd = &plan.cwd;
    if cwd.file_name().and_then(|n| n.to_str()) == Some(".minecraft") {
        if let Some(name) = cwd
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        {
            return name.to_string();
        }
    }
    "instance".into()
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_xdg_dir(paths: &LauncherPaths, plan: &LaunchPlan) -> PathBuf {
    paths
        .root
        .join(format!("cache/xdg-{}", instance_slug(plan)))
}

#[cfg(target_os = "linux")]
fn bwrap_on_path() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join("bwrap").is_file())
}

fn ensure_write_dirs(plan: &LaunchPlan) -> Result<(), EngineError> {
    for dir in &plan.sandbox.allow_write {
        std::fs::create_dir_all(dir).map_err(|e| EngineError::io(dir, e))?;
    }
    Ok(())
}

#[cfg(any(test, target_os = "linux"))]
pub(crate) fn linux_runtime_socket_paths(runtime: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(runtime) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with("wayland-") || name == "pulse" || name == "pipewire-0" {
            out.push(entry.path());
        }
    }
    out
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|p| p == &path) {
        paths.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{fill_spec, sandbox_status};
    use crate::paths::LauncherPaths;
    use crate::types::{LaunchPlan, SandboxSpec, SandboxStatus};
    use std::path::PathBuf;

    #[test]
    fn fill_spec_write_set_is_only_game_and_natives() {
        let paths = LauncherPaths::new(PathBuf::from("/data/kmine"));
        let plan = LaunchPlan {
            java: PathBuf::from(
                "/data/kmine/cache/runtime/java-runtime-delta/mac-os-arm64/bin/java",
            ),
            jvm_args: vec![],
            main_class: "n.m.Main".into(),
            game_args: vec![],
            classpath: vec![],
            natives_dir: PathBuf::from("/data/kmine/cache/natives/aaa"),
            cwd: PathBuf::from("/data/kmine/instances/A/.minecraft"),
            env: vec![],
            sandbox: SandboxSpec {
                enabled: true,
                allow_read: vec![],
                allow_write: vec![],
                network: true,
            },
        };
        let spec = fill_spec(&plan, &paths);
        assert!(spec.allow_write.iter().all(
            |p| p.starts_with("/data/kmine/instances/A/.minecraft")
                || p.starts_with("/data/kmine/cache/natives")
                || p.starts_with("/data/kmine/cache/xdg-")
        ));
        assert!(!spec.allow_write.iter().any(|p| p.ends_with("kmine.db")));
        assert!(
            spec.allow_read
                .iter()
                .any(|p| p.starts_with("/data/kmine/cache/libraries"))
        );
    }

    #[test]
    fn linux_runtime_binds_skip_session_bus_and_keyring() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("wayland-0"), b"").unwrap();
        std::fs::write(dir.path().join("bus"), b"").unwrap();
        std::fs::create_dir(dir.path().join("keyring")).unwrap();
        std::fs::write(dir.path().join("pipewire-0"), b"").unwrap();
        std::fs::create_dir(dir.path().join("pulse")).unwrap();
        let binds = super::linux_runtime_socket_paths(dir.path());
        assert!(binds.iter().any(|p| p.ends_with("wayland-0")));
        assert!(binds.iter().any(|p| p.ends_with("pipewire-0")));
        assert!(binds.iter().any(|p| p.ends_with("pulse")));
        assert!(!binds.iter().any(|p| p.ends_with("bus")));
        assert!(!binds.iter().any(|p| p.ends_with("keyring")));
        assert!(!binds.iter().any(|p| p == dir.path()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn write_profile_allows_mapping_natives() {
        let text = super::macos::profile_source(1, 1, true);
        assert!(
            text.contains("file-map-executable")
                && text.contains("WRITE_0")
                && text.contains("file-write*")
        );
        let write_line = text
            .lines()
            .find(|l| l.contains("WRITE_0"))
            .expect("write rule");
        assert!(
            write_line.contains("file-map-executable"),
            "write subpaths must be mappable for LWJGL natives: {write_line}"
        );
    }

    #[test]
    fn sandbox_status_on_this_os() {
        let s = sandbox_status();
        #[cfg(target_os = "linux")]
        {
            let _ = s;
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            assert!(matches!(s, SandboxStatus::Available));
        }
    }
}
