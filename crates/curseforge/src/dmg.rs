use super::extract::{extract_from_bytes, first_key, CfCoreKey};
use std::io::{Cursor, Write};
use udif::reader::{DmgReader, DmgReaderOptions};

pub(crate) fn looks_like_dmg(bytes: &[u8]) -> bool {
    bytes.len() >= 512 && bytes[bytes.len() - 512..].starts_with(b"koly")
}

pub(crate) fn extract_from_dmg(bytes: &[u8]) -> Option<CfCoreKey> {
    let mut reader = DmgReader::with_options(
        Cursor::new(bytes),
        DmgReaderOptions {
            verify_checksums: false,
        },
    )
    .ok()?;
    let mut ids = Vec::new();
    if let Ok(id) = reader.main_partition_id() {
        ids.push(id);
    }
    if let Ok(id) = reader.hfs_partition_id() {
        ids.push(id);
    }
    ids.extend(reader.partitions().iter().map(|p| p.id));
    ids.sort_unstable();
    ids.dedup();

    for id in ids {
        let mut hunt = Hunt::default();
        if reader.decompress_partition_to(id, &mut hunt).is_ok()
            && let Some(found) = hunt.found
        {
            return Some(prefix_source(found));
        }
        if let Ok(part) = reader.decompress_partition(id)
            && let Some(found) = extract_from_bytes(&part)
        {
            return Some(prefix_source(found));
        }
    }
    None
}

fn prefix_source(mut found: CfCoreKey) -> CfCoreKey {
    if !found.source.starts_with("dmg:") {
        found.source = format!("dmg:{}", found.source);
    }
    found
}

/// Overlap scan of a decompressed volume. Only the last 80 bytes are kept.
#[derive(Default)]
struct Hunt {
    tail: Vec<u8>,
    found: Option<CfCoreKey>,
}

impl Hunt {
    fn feed(&mut self, chunk: &[u8]) {
        if self.found.is_some() || chunk.is_empty() {
            return;
        }
        let mut window = std::mem::take(&mut self.tail);
        window.extend_from_slice(chunk);
        if let Some(key) = first_key(&window) {
            self.found = Some(CfCoreKey {
                key,
                source: "dmg:scan".into(),
            });
            return;
        }
        let keep = 80.min(window.len());
        self.tail.clear();
        self.tail.extend_from_slice(&window[window.len() - keep..]);
    }
}

impl Write for Hunt {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.feed(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
