//! AppContainer jail from Microsoft's "Launch an AppContainer" sample:
//! `CreateAppContainerProfile`, capability SIDs, DACLs, `CreateProcess`.

use super::{apply_plan_stdio, instance_slug};
use crate::error::EngineError;
use crate::types::LaunchPlan;
use sha1::{Digest, Sha1};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GENERIC_ALL, GENERIC_EXECUTE, GENERIC_READ, HANDLE,
    LocalFree,
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
    PSID, SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_QUERY_INFORMATION, THREAD_SET_INFORMATION,
    THREAD_SUSPEND_RESUME,
};
use windows::core::{PCWSTR, PWSTR, s, w};

const SE_GROUP_ENABLED: u32 = 4;
const PROCESS_ACCESS_TOKEN: u32 = 9;
const CREATE_SUSPENDED_FLAG: u32 = CREATE_SUSPENDED.0;

#[repr(C)]
struct ProcessAccessToken {
    token: HANDLE,
    thread: HANDLE,
}

type CreateAppContainerTokenFn = unsafe extern "system" fn(
    HANDLE,
    *const SECURITY_CAPABILITIES,
    *mut HANDLE,
) -> windows::core::HRESULT;

type NtSetInformationProcessFn =
    unsafe extern "system" fn(HANDLE, u32, *const ProcessAccessToken, u32) -> i32;

pub(super) fn spawn(plan: &LaunchPlan) -> Result<std::process::Child, EngineError> {
    let name = appcontainer_name(plan);
    let mut session = AppContainer::create(&name, plan.sandbox.network)?;
    session.grant_paths(plan)?;

    let mut cmd = Command::new(&plan.java);
    apply_plan_stdio(&mut cmd, plan);
    cmd.creation_flags(CREATE_SUSPENDED_FLAG);
    let mut child = cmd.spawn().map_err(|e| EngineError::io(&plan.java, e))?;
    if let Err(err) = session.apply_to_child(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }
    if let Err(err) = resume_process(child.id()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err);
    }
    Ok(child)
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

    fn apply_to_child(&mut self, child: &std::process::Child) -> Result<(), EngineError> {
        let process = HANDLE(child.as_raw_handle());
        let thread = primary_thread(child.id())?;
        let mut caps = SECURITY_CAPABILITIES {
            AppContainerSid: self.sid,
            Capabilities: self.capabilities.as_mut_ptr(),
            CapabilityCount: self.capabilities.len() as u32,
            Reserved: 0,
        };
        let create_token = create_appcontainer_token_fn()?;
        let set_token = nt_set_information_process_fn()?;
        let mut token = HANDLE::default();
        let hr = unsafe { create_token(HANDLE::default(), &caps, &mut token) };
        if hr.is_err() {
            let _ = unsafe { CloseHandle(thread) };
            return Err(unavailable(format!(
                "CreateAppContainerToken failed: {hr:?}"
            )));
        }
        let info = ProcessAccessToken { token, thread };
        let status = unsafe {
            set_token(
                process,
                PROCESS_ACCESS_TOKEN,
                &info,
                std::mem::size_of::<ProcessAccessToken>() as u32,
            )
        };
        let _ = unsafe { CloseHandle(token) };
        let _ = unsafe { CloseHandle(thread) };
        if status < 0 {
            return Err(unavailable(format!(
                "NtSetInformationProcess failed: {status:#x}"
            )));
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

fn create_appcontainer_token_fn() -> Result<CreateAppContainerTokenFn, EngineError> {
    unsafe {
        let module =
            GetModuleHandleW(w!("kernelbase.dll")).map_err(|e| unavailable(e.to_string()))?;
        let proc = GetProcAddress(module, s!("CreateAppContainerToken"))
            .ok_or_else(|| unavailable("CreateAppContainerToken not found"))?;
        Ok(std::mem::transmute(proc))
    }
}

fn nt_set_information_process_fn() -> Result<NtSetInformationProcessFn, EngineError> {
    unsafe {
        let module = GetModuleHandleW(w!("ntdll.dll")).map_err(|e| unavailable(e.to_string()))?;
        let proc = GetProcAddress(module, s!("NtSetInformationProcess"))
            .ok_or_else(|| unavailable("NtSetInformationProcess not found"))?;
        Ok(std::mem::transmute(proc))
    }
}

fn primary_thread(pid: u32) -> Result<HANDLE, EngineError> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
            .map_err(|e| unavailable(e.to_string()))?;
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let result = (|| {
            Thread32First(snap, &mut entry).map_err(|e| unavailable(e.to_string()))?;
            loop {
                if entry.th32OwnerProcessID == pid {
                    return OpenThread(
                        THREAD_SUSPEND_RESUME | THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION,
                        false,
                        entry.th32ThreadID,
                    )
                    .map_err(|e| unavailable(e.to_string()));
                }
                if Thread32Next(snap, &mut entry).is_err() {
                    break;
                }
            }
            Err(unavailable("AppContainer process has no thread"))
        })();
        let _ = CloseHandle(snap);
        result
    }
}

fn resume_process(pid: u32) -> Result<(), EngineError> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
            .map_err(|e| unavailable(e.to_string()))?;
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let result = (|| {
            Thread32First(snap, &mut entry).map_err(|e| unavailable(e.to_string()))?;
            let mut resumed = false;
            loop {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID)
                        .map_err(|e| unavailable(e.to_string()))?;
                    let prev = ResumeThread(thread);
                    let _ = CloseHandle(thread);
                    if prev == u32::MAX {
                        return Err(unavailable("ResumeThread failed"));
                    }
                    resumed = true;
                }
                if Thread32Next(snap, &mut entry).is_err() {
                    break;
                }
            }
            if resumed {
                Ok(())
            } else {
                Err(unavailable("no thread to resume"))
            }
        })();
        let _ = CloseHandle(snap);
        result
    }
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

fn unavailable(reason: impl Into<String>) -> EngineError {
    EngineError::SandboxUnavailable {
        reason: reason.into(),
    }
}
