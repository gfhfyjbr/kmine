use serde::Deserialize;
use std::collections::BTreeMap;

/// Electron asar: 8-byte size pickle + header pickle + concatenated file bytes.
/// Files are not compressed, so JS is plain text once you have the slice.
pub(super) struct Asar<'a> {
    data_base: usize,
    bytes: &'a [u8],
    files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Deserialize)]
struct Header {
    files: BTreeMap<String, Node>,
}

#[derive(Debug, Deserialize)]
struct Node {
    #[serde(default)]
    files: Option<BTreeMap<String, Node>>,
    #[serde(default)]
    offset: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    unpacked: bool,
}

struct FileEntry {
    offset: u64,
    size: u64,
}

impl<'a> Asar<'a> {
    pub(super) fn parse(bytes: &'a [u8]) -> Option<Self> {
        parse_at(bytes, 0)
    }

    pub(super) fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files.iter().filter_map(|(path, entry)| {
            let start = self.data_base.checked_add(entry.offset as usize)?;
            let end = start.checked_add(entry.size as usize)?;
            let slice = self.bytes.get(start..end)?;
            Some((path.as_str(), slice))
        })
    }

    fn end(&self) -> usize {
        self.files
            .values()
            .filter_map(|entry| {
                self.data_base
                    .checked_add(entry.offset as usize)?
                    .checked_add(entry.size as usize)
            })
            .max()
            .unwrap_or(self.data_base)
    }
}

pub(super) fn find_asars(bytes: &[u8]) -> Vec<Asar<'_>> {
    let mut out = Vec::new();
    if let Some(asar) = Asar::parse(bytes) {
        out.push(asar);
        return out;
    }
    let mut from = 0;
    while let Some(rel) = find_files_json(&bytes[from..]) {
        let abs = from + rel;
        if abs >= 16
            && let Some(asar) = parse_at(bytes, abs - 16)
        {
            from = asar.end().max(abs + 1);
            out.push(asar);
            continue;
        }
        from = abs + 1;
    }
    out
}

fn parse_at(whole: &[u8], start: usize) -> Option<Asar<'_>> {
    let bytes = whole.get(start..)?;
    if bytes.len() < 16 || u32_le(bytes, 0)? != 4 {
        return None;
    }
    let header_size = u32_le(bytes, 4)? as usize;
    let json_len = u32_le(bytes, 12)? as usize;
    let json_end = 16usize.checked_add(json_len)?;
    let header_end = 8usize.checked_add(header_size)?;
    if json_end > header_end || header_end > bytes.len() {
        return None;
    }
    let header: Header = serde_json::from_slice(&bytes[16..json_end]).ok()?;
    let mut files = BTreeMap::new();
    flatten(&header.files, String::new(), &mut files);
    if files.is_empty() {
        return None;
    }
    Some(Asar {
        data_base: start + header_end,
        bytes: whole,
        files,
    })
}

fn flatten(tree: &BTreeMap<String, Node>, prefix: String, out: &mut BTreeMap<String, FileEntry>) {
    for (name, node) in tree {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if let Some(children) = &node.files {
            flatten(children, path, out);
            continue;
        }
        if node.unpacked {
            continue;
        }
        let (Some(offset), Some(size)) = (&node.offset, node.size) else {
            continue;
        };
        let Ok(offset) = offset.parse::<u64>() else {
            continue;
        };
        out.insert(path, FileEntry { offset, size });
    }
}

fn find_files_json(bytes: &[u8]) -> Option<usize> {
    const NEEDLE: &[u8] = b"{\"files\":";
    bytes.windows(NEEDLE.len()).position(|w| w == NEEDLE)
}

fn u32_le(bytes: &[u8], off: usize) -> Option<u32> {
    bytes
        .get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
}

#[cfg(test)]
pub(super) fn pack(files: &[(&str, &[u8])]) -> Vec<u8> {
    use serde_json::{Map, Value};

    fn insert(tree: &mut Map<String, Value>, path: &str, offset: u64, size: usize) {
        let mut parts = path.split('/').filter(|s| !s.is_empty()).peekable();
        let mut cur = tree;
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                let mut meta = Map::new();
                meta.insert("offset".into(), Value::String(offset.to_string()));
                meta.insert("size".into(), Value::from(size));
                cur.insert(part.to_string(), Value::Object(meta));
                return;
            }
            let entry = cur.entry(part.to_string()).or_insert_with(|| {
                Value::Object({
                    let mut m = Map::new();
                    m.insert("files".into(), Value::Object(Map::new()));
                    m
                })
            });
            let obj = entry.as_object_mut().expect("dir");
            let files = obj
                .entry("files")
                .or_insert_with(|| Value::Object(Map::new()));
            cur = files.as_object_mut().expect("files");
        }
    }

    let mut tree = Map::new();
    let mut blob = Vec::new();
    for (path, bytes) in files {
        insert(&mut tree, path, blob.len() as u64, bytes.len());
        blob.extend_from_slice(bytes);
    }
    let mut root = Map::new();
    root.insert("files".into(), Value::Object(tree));
    let json = serde_json::to_vec(&Value::Object(root)).expect("json");
    let json_len = json.len();
    let pad = (4 - (json_len % 4)) % 4;
    let pickle_payload = 4 + json_len + pad;
    let header_size = 4 + pickle_payload;

    let mut out = Vec::with_capacity(8 + header_size + blob.len());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&(header_size as u32).to_le_bytes());
    out.extend_from_slice(&(pickle_payload as u32).to_le_bytes());
    out.extend_from_slice(&(json_len as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend(std::iter::repeat_n(0u8, pad));
    out.extend_from_slice(&blob);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_one_file() {
        let bytes = pack(&[("dist/background/background.js", b"hello")]);
        let asar = Asar::parse(&bytes).expect("parse");
        let files: Vec<_> = asar.files().collect();
        assert_eq!(
            files,
            vec![("dist/background/background.js", b"hello".as_slice())]
        );
    }

    #[test]
    fn finds_asar_inside_noise() {
        let inner = pack(&[("a.js", b"abc")]);
        let mut blob = b"xxxx".to_vec();
        blob.extend_from_slice(&inner);
        blob.extend_from_slice(b"yyyy");
        let found = find_asars(&blob);
        assert_eq!(found.len(), 1);
        let files: Vec<_> = found[0].files().collect();
        assert_eq!(files[0].1, b"abc");
    }
}
