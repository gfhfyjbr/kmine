use crate::error::EngineError;
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload},
};
use rand::RngCore;

pub const MASTER_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

pub fn generate_master_key() -> [u8; MASTER_KEY_LEN] {
    let mut key = [0u8; MASTER_KEY_LEN];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

pub fn seal(key: &[u8; 32], id: &str, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), EngineError> {
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(
            GenericArray::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: id.as_bytes(),
            },
        )
        .map_err(|_| EngineError::Crypto)?;
    Ok((nonce.to_vec(), ct))
}

pub fn open(
    key: &[u8; 32],
    id: &str,
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, EngineError> {
    if nonce.len() != NONCE_LEN {
        return Err(EngineError::Crypto);
    }
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    cipher
        .decrypt(
            GenericArray::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: id.as_bytes(),
            },
        )
        .map_err(|_| EngineError::Crypto)
}

#[cfg(test)]
mod tests {
    use super::{generate_master_key, open, seal};

    #[test]
    fn seal_open_round_trip() {
        let key = generate_master_key();
        let (nonce, ct) = seal(&key, "account/u1", b"hello").unwrap();
        assert_eq!(nonce.len(), 12);
        let pt = open(&key, "account/u1", &nonce, &ct).unwrap();
        assert_eq!(pt, b"hello");
    }

    #[test]
    fn wrong_aad_fails() {
        let key = generate_master_key();
        let (nonce, ct) = seal(&key, "account/u1", b"hello").unwrap();
        assert!(open(&key, "account/u2", &nonce, &ct).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let (nonce, ct) = seal(&generate_master_key(), "id", b"x").unwrap();
        assert!(open(&generate_master_key(), "id", &nonce, &ct).is_err());
    }
}
