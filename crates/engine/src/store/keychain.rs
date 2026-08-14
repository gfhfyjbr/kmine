use crate::error::EngineError;
use std::sync::Mutex;

const KEYCHAIN_SERVICE: &str = "dev.kmine.launcher";
const KEYCHAIN_ACCOUNT: &str = "master-key";

pub trait Keychain: Send + Sync {
    fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError>;
    fn set_master_key(&self, key: &[u8; 32]) -> Result<(), EngineError>;
}

pub struct MemoryKeychain {
    key: Mutex<Option<[u8; 32]>>,
}

impl MemoryKeychain {
    pub fn new() -> Self {
        Self {
            key: Mutex::new(None),
        }
    }
}

impl Keychain for MemoryKeychain {
    fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError> {
        Ok(*self.key.lock().expect("keychain mutex"))
    }
    fn set_master_key(&self, key: &[u8; 32]) -> Result<(), EngineError> {
        *self.key.lock().expect("keychain mutex") = Some(*key);
        Ok(())
    }
}

pub struct OsKeychain;

impl OsKeychain {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE, Keychain, OsKeychain};
    use crate::error::EngineError;
    use security_framework::os::macos::keychain::SecKeychain;

    /// `errSecItemNotFound` from Security.framework.
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    fn master_key_from_bytes(bytes: &[u8]) -> Result<[u8; 32], EngineError> {
        bytes.try_into().map_err(|_| EngineError::Crypto)
    }

    impl Keychain for OsKeychain {
        fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError> {
            let keychain = SecKeychain::default().map_err(|_| EngineError::Crypto)?;
            match keychain.find_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
                Ok((password, _item)) => Ok(Some(master_key_from_bytes(password.as_ref())?)),
                Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
                Err(_) => Err(EngineError::Crypto),
            }
        }

        fn set_master_key(&self, key: &[u8; 32]) -> Result<(), EngineError> {
            let keychain = SecKeychain::default().map_err(|_| EngineError::Crypto)?;
            keychain
                .set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, key)
                .map_err(|_| EngineError::Crypto)
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE, Keychain, OsKeychain};
    use crate::error::EngineError;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{ERROR_NOT_FOUND, FILETIME};
    use windows::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW, CredWriteW,
    };
    use windows::core::{PCWSTR, PWSTR};

    fn target_name() -> String {
        format!("{KEYCHAIN_SERVICE}/{KEYCHAIN_ACCOUNT}")
    }

    fn target_wstr() -> Vec<u16> {
        OsStr::new(&target_name())
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn is_not_found(err: &windows::core::Error) -> bool {
        err.code() == ERROR_NOT_FOUND.to_hresult()
    }

    impl Keychain for OsKeychain {
        fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError> {
            let target = target_wstr();
            let mut credential = std::ptr::null_mut();
            let result = unsafe {
                CredReadW(
                    PCWSTR(target.as_ptr()),
                    CRED_TYPE_GENERIC,
                    None,
                    &mut credential,
                )
            };
            match result {
                Ok(()) => {
                    let key = unsafe {
                        let cred = &*credential;
                        let bytes = std::slice::from_raw_parts(
                            cred.CredentialBlob,
                            cred.CredentialBlobSize as usize,
                        );
                        let parsed = <[u8; 32]>::try_from(bytes).map_err(|_| EngineError::Crypto);
                        CredFree(credential.cast());
                        parsed
                    }?;
                    Ok(Some(key))
                }
                Err(err) if is_not_found(&err) => Ok(None),
                Err(_) => Err(EngineError::Crypto),
            }
        }

        fn set_master_key(&self, key: &[u8; 32]) -> Result<(), EngineError> {
            let mut target = target_wstr();
            let credential = CREDENTIALW {
                Flags: Default::default(),
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(target.as_mut_ptr()),
                Comment: PWSTR::null(),
                LastWritten: FILETIME {
                    dwLowDateTime: 0,
                    dwHighDateTime: 0,
                },
                CredentialBlobSize: key.len() as u32,
                CredentialBlob: key.as_ptr() as *mut u8,
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                AttributeCount: 0,
                Attributes: std::ptr::null_mut(),
                TargetAlias: PWSTR::null(),
                UserName: PWSTR::null(),
            };
            unsafe { CredWriteW(&credential, 0) }.map_err(|_| EngineError::Crypto)
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE, Keychain, OsKeychain};
    use crate::error::EngineError;

    const ITEM_LABEL: &str = "kmine launcher master key";

    fn attributes() -> [(&'static str, &'static str); 2] {
        [("service", KEYCHAIN_SERVICE), ("account", KEYCHAIN_ACCOUNT)]
    }

    impl Keychain for OsKeychain {
        fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError> {
            pollster::block_on(async {
                let keyring = oo7::Keyring::new().await.map_err(|_| EngineError::Crypto)?;
                let items = keyring
                    .search_items(&attributes())
                    .await
                    .map_err(|_| EngineError::Crypto)?;
                let Some(item) = items.into_iter().next() else {
                    return Ok(None);
                };
                let secret = item.secret().await.map_err(|_| EngineError::Crypto)?;
                let key =
                    <[u8; 32]>::try_from(secret.as_bytes()).map_err(|_| EngineError::Crypto)?;
                Ok(Some(key))
            })
        }

        fn set_master_key(&self, key: &[u8; 32]) -> Result<(), EngineError> {
            pollster::block_on(async {
                let keyring = oo7::Keyring::new().await.map_err(|_| EngineError::Crypto)?;
                keyring
                    .create_item(ITEM_LABEL, &attributes(), key.as_slice(), true)
                    .await
                    .map_err(|_| EngineError::Crypto)
            })
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod other {
    use super::{Keychain, OsKeychain};
    use crate::error::EngineError;

    impl Keychain for OsKeychain {
        fn get_master_key(&self) -> Result<Option<[u8; 32]>, EngineError> {
            Err(EngineError::Crypto)
        }

        fn set_master_key(&self, _key: &[u8; 32]) -> Result<(), EngineError> {
            Err(EngineError::Crypto)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Keychain, MemoryKeychain};

    #[test]
    fn memory_keychain_persists_in_process() {
        let kc = MemoryKeychain::new();
        assert!(kc.get_master_key().unwrap().is_none());
        let key = [7u8; 32];
        kc.set_master_key(&key).unwrap();
        assert_eq!(kc.get_master_key().unwrap(), Some(key));
    }
}
