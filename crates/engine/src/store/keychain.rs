use crate::error::EngineError;
use std::sync::Mutex;

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
