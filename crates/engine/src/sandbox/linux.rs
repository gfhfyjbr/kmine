//! bubblewrap jail from the `bwrap(1)` filesystem/namespace options.

use super::apply_plan_stdio;
use crate::error::EngineError;
use crate::types::LaunchPlan;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn spawn(plan: &LaunchPlan) -> Result<std::process::Child, EngineError> {
    let bwrap = find_bwrap().ok_or_else(|| EngineError::SandboxUnavailable {
        reason: "bwrap not found on PATH".into(),
    })?;

    let mut cmd = Command::new(&bwrap);
    cmd.arg("--unshare-all");
    if plan.sandbox.network {
        cmd.arg("--share-net");
    }
    cmd.arg("--proc").arg("/proc");
    cmd.arg("--dev").arg("/dev");
    if Path::new("/dev/dri").exists() {
        cmd.arg("--dev-bind").arg("/dev/dri").arg("/dev/dri");
    }
    cmd.arg("--die-with-parent");
    cmd.arg("--new-session");

    for host in [
        "/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/sys", "/opt",
    ] {
        ro_bind_try(&mut cmd, Path::new(host));
    }
    cmd.arg("--tmpfs").arg("/tmp");

    if Path::new("/dev/snd").exists() {
        cmd.arg("--dev-bind").arg("/dev/snd").arg("/dev/snd");
    }
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        bind_try(&mut cmd, Path::new(&runtime));
    }
    ro_bind_try(&mut cmd, Path::new("/tmp/.X11-unix"));
    if let Ok(xauth) = std::env::var("XAUTHORITY") {
        ro_bind_try(&mut cmd, Path::new(&xauth));
    }

    for path in &plan.sandbox.allow_read {
        ro_bind_try(&mut cmd, path);
    }
    for path in &plan.sandbox.allow_write {
        bind_try(&mut cmd, path);
    }

    cmd.arg("--chdir").arg(&plan.cwd);
    let xdg = plan
        .sandbox
        .allow_write
        .iter()
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("xdg-"))
        })
        .cloned()
        .unwrap_or_else(|| plan.cwd.clone());
    cmd.arg("--setenv").arg("HOME").arg(&xdg);
    cmd.arg("--setenv").arg("XDG_CACHE_HOME").arg(&xdg);
    cmd.arg("--setenv").arg("XDG_CONFIG_HOME").arg(&xdg);
    cmd.arg("--setenv").arg("XDG_DATA_HOME").arg(&xdg);
    cmd.arg("--setenv").arg("TMPDIR").arg(&plan.cwd);

    cmd.arg(&plan.java);
    apply_plan_stdio(&mut cmd, plan);
    cmd.spawn().map_err(|e| EngineError::io(&bwrap, e))
}

fn find_bwrap() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let cand = dir.join("bwrap");
        cand.is_file().then_some(cand)
    })
}

fn ro_bind_try(cmd: &mut Command, path: &Path) {
    if path.as_os_str().is_empty() {
        return;
    }
    cmd.arg("--ro-bind-try").arg(path).arg(path);
}

fn bind_try(cmd: &mut Command, path: &Path) {
    if path.as_os_str().is_empty() {
        return;
    }
    cmd.arg("--bind-try").arg(path).arg(path);
}
