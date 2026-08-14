//! Seatbelt profile authored from Apple `sandbox_init(3)` and the system
//! SBPL shipped in `/System/Library/Sandbox/Profiles/system.sb`.

use super::{apply_plan_stdio, java_home};
use crate::error::EngineError;
use crate::types::LaunchPlan;
use std::ffi::{CStr, CString, c_char};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

unsafe extern "C" {
    fn sandbox_init_with_parameters(
        profile: *const c_char,
        flags: u64,
        parameters: *const *const c_char,
        errorbuf: *mut *mut c_char,
    ) -> i32;
    fn sandbox_free_error(errorbuf: *mut c_char);
}

pub(super) fn spawn(plan: &LaunchPlan) -> Result<std::process::Child, EngineError> {
    let (profile, params) = seatbelt_profile(plan)?;
    let mut cmd = Command::new(&plan.java);
    apply_plan_stdio(&mut cmd, plan);
    unsafe {
        cmd.pre_exec(move || apply_seatbelt(&profile, &params));
    }
    cmd.spawn().map_err(|e| EngineError::io(&plan.java, e))
}

fn seatbelt_profile(plan: &LaunchPlan) -> Result<(CString, Vec<CString>), EngineError> {
    let java_home = java_home(&plan.java);
    let reads: Vec<&Path> = plan.sandbox.allow_read.iter().map(Path::new).collect();
    let writes: Vec<&Path> = plan.sandbox.allow_write.iter().map(Path::new).collect();

    let mut text = String::from(
        "(version 1)\n\
         (deny default)\n\
         (import \"system.sb\")\n\
         (system-graphics)\n\
         (allow syscall*)\n\
         (allow mach-bootstrap)\n\
         (allow process-fork)\n\
         (allow process-info*)\n\
         (allow signal (target self))\n\
         (allow sysctl-read)\n\
         (allow ipc-posix-shm*)\n\
         (allow file-read-metadata file-test-existence (subpath \"/\"))\n\
         (allow mach-lookup\n\
           (global-name \"com.apple.windowserver.active\")\n\
           (global-name \"com.apple.windowmanager.server\")\n\
           (global-name \"com.apple.fonts\")\n\
           (global-name-prefix \"com.apple.audio.\"))\n\
         (allow iokit-open-user-client\n\
           (iokit-user-client-class \"IOHIDLibUserClient\" \"IOHIDParamUserClient\"))\n\
         (allow process-exec* (literal (param \"JAVA\")) (subpath (param \"JAVA_HOME\")))\n\
         (allow file-map-executable (subpath (param \"JAVA_HOME\")))\n",
    );
    for i in 0..reads.len() {
        text.push_str(&format!(
            "(allow file-read* file-map-executable file-test-existence (subpath (param \"READ_{i}\")))\n"
        ));
    }
    for i in 0..writes.len() {
        text.push_str(&format!(
            "(allow file-read* file-write* file-ioctl file-test-existence (subpath (param \"WRITE_{i}\")))\n"
        ));
    }
    if plan.sandbox.network {
        text.push_str("(system-network)\n(allow network*)\n");
    }

    let profile = cstring(&text)?;
    let mut params = Vec::new();
    push_param(&mut params, "JAVA", plan.java.as_path())?;
    push_param(&mut params, "JAVA_HOME", &java_home)?;
    for (i, path) in reads.iter().enumerate() {
        push_param(&mut params, &format!("READ_{i}"), path)?;
    }
    for (i, path) in writes.iter().enumerate() {
        push_param(&mut params, &format!("WRITE_{i}"), path)?;
    }
    Ok((profile, params))
}

fn push_param(params: &mut Vec<CString>, key: &str, path: &Path) -> Result<(), EngineError> {
    params.push(cstring(key)?);
    params.push(cstring(&path.to_string_lossy())?);
    Ok(())
}

fn cstring(s: &str) -> Result<CString, EngineError> {
    CString::new(s).map_err(|e| {
        EngineError::io(
            Path::new("sandbox"),
            io::Error::new(io::ErrorKind::InvalidInput, e),
        )
    })
}

fn apply_seatbelt(profile: &CStr, params: &[CString]) -> io::Result<()> {
    let mut ptrs: Vec<*const c_char> = params.iter().map(|s| s.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    let mut err: *mut c_char = std::ptr::null_mut();
    let rc = unsafe { sandbox_init_with_parameters(profile.as_ptr(), 0, ptrs.as_ptr(), &mut err) };
    if rc == 0 {
        return Ok(());
    }
    let message = if err.is_null() {
        "sandbox_init_with_parameters failed".into()
    } else {
        let owned = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { sandbox_free_error(err) };
        owned
    };
    Err(io::Error::other(message))
}
