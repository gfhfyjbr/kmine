//! CurseForge file fingerprint: whitespace-stripped MurmurHash2 (32-bit, seed 1).

use crate::types::File;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct FingerprintMatches {
    pub exact: Vec<FingerprintMatch>,
    pub unmatched: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FingerprintMatch {
    pub id: u32,
    pub file: File,
    pub latest_files: Vec<File>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FingerprintEnvelope {
    #[serde(default)]
    pub exact_matches: Vec<ExactMatch>,
    #[serde(default)]
    pub unmatched_fingerprints: Vec<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExactMatch {
    pub id: u32,
    pub file: File,
    #[serde(default)]
    pub latest_files: Vec<File>,
}

pub fn fingerprint(bytes: &[u8]) -> u32 {
    let filtered: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !matches!(b, 9 | 10 | 13 | 32))
        .collect();
    murmurhash2(&filtered, 1)
}

fn murmurhash2(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1e995;
    const R: u32 = 24;
    let mut len = data.len();
    let mut h = seed ^ (len as u32);
    let mut i = 0;
    while len >= 4 {
        let mut k = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        k = k.wrapping_mul(M);
        k ^= k >> R;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
        i += 4;
        len -= 4;
    }
    match len {
        3 => {
            h ^= (data[i + 2] as u32) << 16;
            h ^= (data[i + 1] as u32) << 8;
            h ^= data[i] as u32;
            h = h.wrapping_mul(M);
        }
        2 => {
            h ^= (data[i + 1] as u32) << 8;
            h ^= data[i] as u32;
            h = h.wrapping_mul(M);
        }
        1 => {
            h ^= data[i] as u32;
            h = h.wrapping_mul(M);
        }
        _ => {}
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

#[cfg(test)]
mod tests {
    use super::fingerprint;

    #[test]
    fn golden_vectors() {
        assert_eq!(fingerprint(b""), 1540447798);
        assert_eq!(fingerprint(b" \t\r\n"), 1540447798);
        assert_eq!(fingerprint(b"a"), 626045324);
        assert_eq!(fingerprint(b"abcd"), 3376380438);
        assert_eq!(fingerprint(b"hello"), 2788266382);
        assert_eq!(fingerprint(b"he llo\n"), 2788266382);
    }
}
