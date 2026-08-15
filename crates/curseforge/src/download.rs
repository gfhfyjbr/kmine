use crate::types::File;
use bytes::Bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub file_id: u32,
    pub file_name: String,
    pub bytes: Bytes,
    pub sha1: Option<String>,
}

pub fn cdn_file_url(file_id: u32, file_name: &str) -> String {
    let folder = file_id / 1000;
    let leaf = file_id % 1000;
    let name = urlencoding_file_name(file_name);
    format!("https://edge.forgecdn.net/files/{folder}/{leaf}/{name}")
}

fn urlencoding_file_name(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn resolve_download_url(file: &File) -> Option<String> {
    if let Some(url) = file.download_url.as_ref().filter(|u| !u.is_empty()) {
        return Some(url.clone());
    }
    if !file.file_name.is_empty() {
        return Some(cdn_file_url(file.id, &file.file_name));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{File, FileReleaseType};

    fn stub_file(url: Option<&str>, name: &str) -> File {
        File {
            id: 5754631,
            game_id: 432,
            mod_id: 250898,
            is_available: true,
            display_name: name.into(),
            file_name: name.into(),
            release_type: FileReleaseType::Release,
            file_status: 4,
            hashes: vec![],
            file_date: None,
            file_length: 0,
            download_count: 0,
            download_url: url.map(str::to_string),
            game_versions: vec![],
            sortable_game_versions: vec![],
            dependencies: vec![],
            alternate_file_id: None,
            is_server_pack: false,
            server_pack_file_id: None,
            is_early_access_content: false,
            file_fingerprint: 0,
            modules: vec![],
        }
    }

    #[test]
    fn cdn_split() {
        assert_eq!(
            cdn_file_url(5754631, "oreexcavation-1.13.174.jar"),
            "https://edge.forgecdn.net/files/5754/631/oreexcavation-1.13.174.jar"
        );
    }

    #[test]
    fn resolve_prefers_download_url() {
        let f = stub_file(Some("https://cdn.example/a.jar"), "a.jar");
        assert_eq!(
            resolve_download_url(&f).unwrap(),
            "https://cdn.example/a.jar"
        );
        let f = stub_file(None, "oreexcavation-1.13.174.jar");
        assert_eq!(
            resolve_download_url(&f).unwrap(),
            "https://edge.forgecdn.net/files/5754/631/oreexcavation-1.13.174.jar"
        );
        let mut f = stub_file(Some(""), "");
        f.file_name.clear();
        assert!(resolve_download_url(&f).is_none());
    }
}
