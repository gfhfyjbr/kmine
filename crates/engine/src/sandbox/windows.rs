//! AppContainer jail from Microsoft's "Launch an AppContainer" sample:
//! `CreateAppContainerProfile`, capability SIDs, DACLs, `STARTUPINFOEX`
//! + `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`.

use super::instance_slug;
use crate::error::EngineError;
use crate::types::LaunchPlan;
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GENERIC_ALL, GENERIC_EXECUTE, GENERIC_READ, GENERIC_WRITE,
    HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS, LocalFree, SetHandleInformation, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT,
    SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, DeriveCapabilitySidsFromName, FreeSid, PSECURITY_DESCRIPTOR,
    PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES,
    SUB_CONTAINERS_AND_OBJECTS_INHERIT,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION,
    STARTUPINFOEXW, STARTUPINFOW, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

const SE_GROUP_ENABLED: u32 = 4;

static JAILS: parking_lot::Mutex<HashMap<u32, Jail>> = parking_lot::Mutex::new(HashMap::new());

struct Jail {
    java_pid: u32,
    process: HANDLE,
}

unsafe impl Send for Jail {}

pub(super) fn spawn(plan: &LaunchPlan) -> Result<Child, EngineError> {
    let name = appcontainer_name(plan);
    let mut session = AppContainer::create(&name, plan.sandbox.network)?;
    session.grant_paths(plan)?;
    let launched = create_appcontainer_process(plan, &mut session)?;
    adopt_child(launched)
}

pub(super) fn jailed_pid(child: &Child) -> Option<u32> {
    JAILS.lock().get(&child.id()).map(|j| j.java_pid)
}

pub(super) fn wait_jailed(mut child: Child) -> io::Result<ExitStatus> {
    let marker_pid = child.id();
    let jail = JAILS.lock().remove(&marker_pid);
    let Some(jail) = jail else {
        return child.wait();
    };
    let wait = unsafe { WaitForSingleObject(jail.process, INFINITE) };
    if wait != WAIT_OBJECT_0 {
        drop(child);
        return Err(io::Error::last_os_error());
    }
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(jail.process, &mut code) }
        .map_err(|e| io::Error::other(e.to_string()))?;
    let _ = child.wait();
    Ok(ExitStatus::from_raw(code))
}

struct Launched {
    pid: u32,
    process: HANDLE,
    stdout: OwnedHandle,
    stderr: OwnedHandle,
}

fn create_appcontainer_process(
    plan: &LaunchPlan,
    session: &mut AppContainer,
) -> Result<Launched, EngineError> {
    let (stdout_r, stdout_w) = inheritable_pipe()?;
    let (stderr_r, stderr_w) = inheritable_pipe()?;
    let stdin = nul_handle()?;
    unsafe {
        SetHandleInformation(
            HANDLE(stdout_r.as_raw()),
            HANDLE_FLAG_INHERIT.0,
            HANDLE_FLAGS(0),
        )
        .map_err(|e| unavailable(e.to_string()))?;
        SetHandleInformation(
            HANDLE(stderr_r.as_raw()),
            HANDLE_FLAG_INHERIT.0,
            HANDLE_FLAGS(0),
        )
        .map_err(|e| unavailable(e.to_string()))?;
    }

    let mut caps = SECURITY_CAPABILITIES {
        AppContainerSid: session.sid,
        Capabilities: session.capabilities.as_mut_ptr(),
        CapabilityCount: session.capabilities.len() as u32,
        Reserved: 0,
    };

    let mut attr_size = 0usize;
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut attr_size) };
    if attr_size == 0 {
        return Err(unavailable("InitializeProcThreadAttributeList size failed"));
    }
    let mut attr_buf = vec![0u8; attr_size];
    let attr_list = LPPROC_THREAD_ATTRIBUTE_LIST(attr_buf.as_mut_ptr().cast());
    unsafe { InitializeProcThreadAttributeList(Some(attr_list), 1, None, &mut attr_size) }
        .map_err(|e| unavailable(e.to_string()))?;
    let update = unsafe {
        UpdateProcThreadAttribute(
            attr_list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            Some((&raw const caps).cast()),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            None,
            None,
        )
    };
    if let Err(err) = update {
        unsafe { DeleteProcThreadAttributeList(attr_list) };
        return Err(unavailable(err.to_string()));
    }

    let mut si = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
            dwFlags: windows::Win32::System::Threading::STARTF_USESTDHANDLES,
            hStdInput: stdin,
            hStdOutput: stdout_w,
            hStdError: stderr_w,
            ..Default::default()
        },
        lpAttributeList: attr_list,
    };

    let exe = wide_path(&plan.java);
    let mut cmdline = wide(&command_line(plan));
    let cwd = wide_path(&plan.cwd);
    let env = env_block(plan);
    let mut pi = PROCESS_INFORMATION::default();
    let created = unsafe {
        CreateProcessW(
            PCWSTR(exe.as_ptr()),
            Some(PWSTR(cmdline.as_mut_ptr())),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            Some(env.as_ptr().cast()),
            PCWSTR(cwd.as_ptr()),
            &si.StartupInfo,
            &mut pi,
        )
    };
    unsafe { DeleteProcThreadAttributeList(attr_list) };
    let _ = unsafe { CloseHandle(stdout_w) };
    let _ = unsafe { CloseHandle(stderr_w) };
    let _ = unsafe { CloseHandle(stdin) };
    created.map_err(|e| unavailable(format!("CreateProcessW AppContainer: {e}")))?;
    let _ = unsafe { CloseHandle(pi.hThread) };

    Ok(Launched {
        pid: pi.dwProcessId,
        process: pi.hProcess,
        stdout: stdout_r,
        stderr: stderr_r,
    })
}

fn adopt_child(launched: Launched) -> Result<Child, EngineError> {
    let mut marker = Command::new("cmd.exe")
        .args(["/C", "exit", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map_err(|e| EngineError::io(Path::new("cmd.exe"), e))?;
    JAILS.lock().insert(
        marker.id(),
        Jail {
            java_pid: launched.pid,
            process: launched.process,
        },
    );
    marker.stdout = Some(ChildStdout::from(launched.stdout));
    marker.stderr = Some(ChildStderr::from(launched.stderr));
    Ok(marker)
}

struct AppContainer {
    sid: PSID,
    capabilities: Vec<SID_AND_ATTRIBUTES>,
    capability_sids: Vec<PSID>,
}

impl AppContainer {
    fn create(name: &str, network: bool) -> Result<Self, EngineError> {
        let name_w = wide(name);
        let display = wide("kmine sandbox");
        let desc = wide("kmine game process AppContainer");
        let sid = unsafe {
            match CreateAppContainerProfile(
                PCWSTR(name_w.as_ptr()),
                PCWSTR(display.as_ptr()),
                PCWSTR(desc.as_ptr()),
                None,
            ) {
                Ok(sid) => sid,
                Err(err) if err.code() == ERROR_ALREADY_EXISTS.to_hresult() => {
                    DeriveAppContainerSidFromAppContainerName(PCWSTR(name_w.as_ptr()))
                        .map_err(|e| unavailable(e.to_string()))?
                }
                Err(err) => return Err(unavailable(err.to_string())),
            }
        };

        let mut capability_sids = Vec::new();
        if network {
            for cap in ["internetClient", "internetClientServer"] {
                match capability_sid(cap) {
                    Ok(sid) => capability_sids.push(sid),
                    Err(reason) => {
                        unsafe {
                            FreeSid(sid);
                        }
                        for extra in capability_sids {
                            unsafe {
                                FreeSid(extra);
                            }
                        }
                        return Err(unavailable(reason));
                    }
                }
            }
        }
        let capabilities = capability_sids
            .iter()
            .map(|sid| SID_AND_ATTRIBUTES {
                Sid: *sid,
                Attributes: SE_GROUP_ENABLED,
            })
            .collect();
        Ok(Self {
            sid,
            capabilities,
            capability_sids,
        })
    }

    fn grant_paths(&self, plan: &LaunchPlan) -> Result<(), EngineError> {
        for path in &plan.sandbox.allow_read {
            grant_acl(path, self.sid, GENERIC_READ.0 | GENERIC_EXECUTE.0)?;
        }
        for path in &plan.sandbox.allow_write {
            grant_acl(path, self.sid, GENERIC_ALL.0)?;
        }
        Ok(())
    }
}

impl Drop for AppContainer {
    fn drop(&mut self) {
        unsafe {
            FreeSid(self.sid);
            for sid in self.capability_sids.drain(..) {
                FreeSid(sid);
            }
        }
    }
}

fn appcontainer_name(plan: &LaunchPlan) -> String {
    let mut hasher = Sha1::new();
    hasher.update(instance_slug(plan).as_bytes());
    let digest = hasher.finalize();
    format!("kmine.{}", hex::encode(&digest[..8]))
}

fn capability_sid(name: &str) -> Result<PSID, String> {
    let name_w = wide(name);
    let mut group_sids = std::ptr::null_mut();
    let mut group_count = 0u32;
    let mut cap_sids = std::ptr::null_mut();
    let mut cap_count = 0u32;
    unsafe {
        DeriveCapabilitySidsFromName(
            PCWSTR(name_w.as_ptr()),
            &mut group_sids,
            &mut group_count,
            &mut cap_sids,
            &mut cap_count,
        )
        .map_err(|e| e.to_string())?;
        if !group_sids.is_null() {
            for i in 0..group_count as usize {
                FreeSid(*group_sids.add(i));
            }
            let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(group_sids.cast())));
        }
        if cap_sids.is_null() || cap_count == 0 {
            return Err(format!("no SID for capability {name}"));
        }
        let sid = *cap_sids;
        for i in 1..cap_count as usize {
            FreeSid(*cap_sids.add(i));
        }
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(cap_sids.cast())));
        Ok(sid)
    }
}

fn grant_acl(path: &Path, sid: PSID, access: u32) -> Result<(), EngineError> {
    if path.as_os_str().is_empty() || !path.exists() {
        return Ok(());
    }
    let path_w = wide_path(path);
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    let get = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut dacl),
            None,
            &mut sd,
        )
    };
    if get.is_err() {
        return Err(unavailable(format!(
            "GetNamedSecurityInfoW {}: {get:?}",
            path.display()
        )));
    }
    let ea = EXPLICIT_ACCESS_W {
        grfAccessPermissions: access,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: PWSTR(sid.0.cast()),
        },
    };
    let mut new_acl = std::ptr::null_mut();
    let old = if dacl.is_null() { None } else { Some(dacl) };
    let set_entries = unsafe { SetEntriesInAclW(Some(&[ea]), old, &mut new_acl) };
    if set_entries.is_err() {
        unsafe {
            let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(sd.0)));
        }
        return Err(unavailable(format!(
            "SetEntriesInAclW {}: {set_entries:?}",
            path.display()
        )));
    }
    let set = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(path_w.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(new_acl),
            None,
        )
    };
    unsafe {
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(new_acl.cast())));
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(sd.0)));
    }
    if set.is_err() {
        return Err(unavailable(format!(
            "SetNamedSecurityInfoW {}: {set:?}",
            path.display()
        )));
    }
    Ok(())
}

fn inheritable_pipe() -> Result<(OwnedHandle, HANDLE), EngineError> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    unsafe { CreatePipe(&mut read, &mut write, Some(&sa), 0) }
        .map_err(|e| unavailable(e.to_string()))?;
    let read = unsafe { OwnedHandle::from_raw_handle(read.0) };
    Ok((read, write))
}

fn nul_handle() -> Result<HANDLE, EngineError> {
    let path = wide("\\\\.\\NUL");
    unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| unavailable(e.to_string()))
}

fn command_line(plan: &LaunchPlan) -> String {
    let mut out = String::new();
    append_quoted(&mut out, plan.java.as_os_str());
    for arg in &plan.jvm_args {
        out.push(' ');
        append_quoted(&mut out, OsStr::new(arg));
    }
    out.push(' ');
    append_quoted(&mut out, OsStr::new(&plan.main_class));
    for arg in &plan.game_args {
        out.push(' ');
        append_quoted(&mut out, OsStr::new(arg));
    }
    out
}

fn append_quoted(out: &mut String, arg: &OsStr) {
    let s = arg.to_string_lossy();
    if !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c == '"') {
        out.push_str(&s);
        return;
    }
    out.push('"');
    for c in s.chars() {
        if c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

fn env_block(plan: &LaunchPlan) -> Vec<u16> {
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let tmp = plan.cwd.as_os_str();
    for key in ["TMPDIR", "TEMP", "TMP"] {
        if let Some(slot) = vars.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            slot.1 = tmp.to_os_string();
        } else {
            vars.push((OsString::from(key), tmp.to_os_string()));
        }
    }
    for (key, value) in &plan.env {
        if let Some(slot) = vars.iter_mut().find(|(k, _)| k == key) {
            slot.1 = OsString::from(value);
        } else {
            vars.push((OsString::from(key), OsString::from(value)));
        }
    }
    let mut block = Vec::new();
    for (k, v) in vars {
        block.extend(k.encode_wide());
        block.push(u16::from(b'='));
        block.extend(v.encode_wide());
        block.push(0);
    }
    block.push(0);
    block
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

trait AsRawVoid {
    fn as_raw(&self) -> RawHandle;
}

impl AsRawVoid for OwnedHandle {
    fn as_raw(&self) -> RawHandle {
        std::os::windows::io::AsRawHandle::as_raw_handle(self)
    }
}

fn unavailable(reason: impl Into<String>) -> EngineError {
    EngineError::SandboxUnavailable {
        reason: reason.into(),
    }
}

impl Drop for Jail {
    fn drop(&mut self) {
        if !self.process.is_invalid() {
            let _ = unsafe { CloseHandle(self.process) };
        }
    }
}
