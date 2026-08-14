use crate::Engine;
use crate::error::EngineError;
use crate::ids::InstanceId;
use crate::instance_not_found;
use crate::types::{QuickPlayLists, QuickPlayServer, QuickPlayWorld};
use flate2::read::GzDecoder;
use std::borrow::Cow;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const TAG_END: u8 = 0x00;
const TAG_BYTE: u8 = 0x01;
const TAG_SHORT: u8 = 0x02;
const TAG_INT: u8 = 0x03;
const TAG_LONG: u8 = 0x04;
const TAG_FLOAT: u8 = 0x05;
const TAG_DOUBLE: u8 = 0x06;
const TAG_BYTE_ARRAY: u8 = 0x07;
const TAG_STRING: u8 = 0x08;
const TAG_LIST: u8 = 0x09;
const TAG_COMPOUND: u8 = 0x0a;
const TAG_INT_ARRAY: u8 = 0x0b;
const TAG_LONG_ARRAY: u8 = 0x0c;

#[derive(Debug, Clone)]
enum Nbt {
    String(String),
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
}

impl Nbt {
    fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Nbt::String(s) => Some(s),
            _ => None,
        }
    }

    fn as_list(&self) -> Option<&[Nbt]> {
        match self {
            Nbt::List(items) => Some(items),
            _ => None,
        }
    }
}

pub fn parse_servers_dat(bytes: &[u8]) -> Result<Vec<QuickPlayServer>, EngineError> {
    let root = parse_root(bytes)?;
    let Some(items) = root.get("servers").and_then(Nbt::as_list) else {
        return Ok(Vec::new());
    };
    let mut servers = Vec::new();
    for item in items {
        let Some(name) = item.get("name").and_then(Nbt::as_str) else {
            continue;
        };
        let Some(ip) = item.get("ip").and_then(Nbt::as_str) else {
            continue;
        };
        servers.push(QuickPlayServer {
            name: name.to_string(),
            address: ip.to_string(),
        });
    }
    Ok(servers)
}

pub fn read_level_name(level_dat: &[u8]) -> Option<String> {
    let root = parse_root(level_dat).ok()?;
    root.get("Data")
        .and_then(|data| data.get("LevelName"))
        .and_then(Nbt::as_str)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

impl Engine {
    pub fn list_quick_play(&self, id: InstanceId) -> Result<QuickPlayLists, EngineError> {
        let slug = {
            let store = self.store.lock();
            match store.get_instance(id)? {
                Some(row) => row.slug,
                None => return Err(instance_not_found(&self.paths)),
            }
        };
        let mc = self.paths.instance_minecraft(&slug);
        Ok(QuickPlayLists {
            worlds: list_worlds(&mc.join("saves"))?,
            servers: list_servers(&mc.join("servers.dat"))?,
        })
    }
}

fn list_worlds(saves: &Path) -> Result<Vec<QuickPlayWorld>, EngineError> {
    let read = match std::fs::read_dir(saves) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(EngineError::io(saves, e)),
    };
    let mut worlds = Vec::new();
    for ent in read {
        let ent = ent.map_err(|e| EngineError::io(saves, e))?;
        let os_name = ent.file_name();
        let Some(folder) = os_name.to_str() else {
            continue;
        };
        let file_type = ent
            .file_type()
            .map_err(|e| EngineError::io(ent.path(), e))?;
        if !file_type.is_dir() {
            continue;
        }
        let level = ent.path().join("level.dat");
        if !level.is_file() {
            continue;
        }
        let bytes = std::fs::read(&level).map_err(|e| EngineError::io(&level, e))?;
        let label = read_level_name(&bytes).unwrap_or_else(|| folder.to_string());
        worlds.push(QuickPlayWorld {
            folder: folder.to_string(),
            label,
        });
    }
    worlds.sort_by(|a, b| a.folder.cmp(&b.folder));
    Ok(worlds)
}

fn list_servers(path: &Path) -> Result<Vec<QuickPlayServer>, EngineError> {
    match std::fs::read(path) {
        Ok(bytes) => parse_servers_dat(&bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(EngineError::io(path, e)),
    }
}

fn parse_root(bytes: &[u8]) -> Result<Nbt, EngineError> {
    let data = inflate_if_gzip(bytes)?;
    let mut r = Reader {
        data: data.as_ref(),
        pos: 0,
    };
    let tag = r.u8()?;
    if tag != TAG_COMPOUND {
        return Err(nbt_err("root is not a compound"));
    }
    let _name = r.mutf8()?;
    r.compound()
}

fn inflate_if_gzip(bytes: &[u8]) -> Result<Cow<'_, [u8]>, EngineError> {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut decoder = GzDecoder::new(bytes);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| EngineError::io(PathBuf::from("nbt"), e))?;
        Ok(Cow::Owned(out))
    } else {
        Ok(Cow::Borrowed(bytes))
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], EngineError> {
        if self.remaining() < n {
            return Err(nbt_err("truncated nbt"));
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.data[start..self.pos])
    }

    fn u8(&mut self) -> Result<u8, EngineError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, EngineError> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn i32(&mut self) -> Result<i32, EngineError> {
        let bytes = self.take(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn mutf8(&mut self) -> Result<String, EngineError> {
        let len = usize::from(self.u16()?);
        decode_mutf8(self.take(len)?)
    }

    fn compound(&mut self) -> Result<Nbt, EngineError> {
        let mut entries = Vec::new();
        loop {
            let tag = self.u8()?;
            if tag == TAG_END {
                break;
            }
            let name = self.mutf8()?;
            if let Some(value) = self.payload(tag)? {
                entries.push((name, value));
            }
        }
        Ok(Nbt::Compound(entries))
    }

    fn list(&mut self) -> Result<Nbt, EngineError> {
        let etype = self.u8()?;
        let count = self.i32()?;
        if count < 0 {
            return Err(nbt_err("negative list length"));
        }
        let mut items = Vec::new();
        for _ in 0..count {
            if let Some(value) = self.payload(etype)? {
                items.push(value);
            }
        }
        Ok(Nbt::List(items))
    }

    fn payload(&mut self, tag: u8) -> Result<Option<Nbt>, EngineError> {
        match tag {
            TAG_STRING => Ok(Some(Nbt::String(self.mutf8()?))),
            TAG_LIST => Ok(Some(self.list()?)),
            TAG_COMPOUND => Ok(Some(self.compound()?)),
            TAG_END => Err(nbt_err("unexpected end tag")),
            other => {
                self.skip_payload(other)?;
                Ok(None)
            }
        }
    }

    fn skip_payload(&mut self, tag: u8) -> Result<(), EngineError> {
        match tag {
            TAG_END => Err(nbt_err("unexpected end tag")),
            TAG_BYTE => {
                self.take(1)?;
                Ok(())
            }
            TAG_SHORT => {
                self.take(2)?;
                Ok(())
            }
            TAG_INT | TAG_FLOAT => {
                self.take(4)?;
                Ok(())
            }
            TAG_LONG | TAG_DOUBLE => {
                self.take(8)?;
                Ok(())
            }
            TAG_BYTE_ARRAY => {
                let len = self.array_len()?;
                self.take(len)?;
                Ok(())
            }
            TAG_STRING => {
                let len = usize::from(self.u16()?);
                self.take(len)?;
                Ok(())
            }
            TAG_LIST => {
                let etype = self.u8()?;
                let count = self.i32()?;
                if count < 0 {
                    return Err(nbt_err("negative list length"));
                }
                for _ in 0..count {
                    self.skip_payload(etype)?;
                }
                Ok(())
            }
            TAG_COMPOUND => {
                loop {
                    let inner = self.u8()?;
                    if inner == TAG_END {
                        break;
                    }
                    let len = usize::from(self.u16()?);
                    self.take(len)?;
                    self.skip_payload(inner)?;
                }
                Ok(())
            }
            TAG_INT_ARRAY => {
                let len = self.array_len()?;
                self.take(len.saturating_mul(4))?;
                Ok(())
            }
            TAG_LONG_ARRAY => {
                let len = self.array_len()?;
                self.take(len.saturating_mul(8))?;
                Ok(())
            }
            _ => Err(nbt_err("unknown nbt tag")),
        }
    }

    fn array_len(&mut self) -> Result<usize, EngineError> {
        let len = self.i32()?;
        if len < 0 {
            return Err(nbt_err("negative array length"));
        }
        usize::try_from(len).map_err(|_| nbt_err("array too large"))
    }
}

fn decode_mutf8(bytes: &[u8]) -> Result<String, EngineError> {
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        if b0 == 0 {
            return Err(nbt_err("invalid modified utf-8"));
        }
        if b0 < 0x80 {
            out.push(char::from(b0));
            i += 1;
            continue;
        }
        if b0 & 0xE0 == 0xC0 {
            let b1 = bytes
                .get(i + 1)
                .copied()
                .ok_or_else(|| nbt_err("truncated modified utf-8"))?;
            if b1 & 0xC0 != 0x80 {
                return Err(nbt_err("invalid modified utf-8"));
            }
            let cp = (u32::from(b0 & 0x1F) << 6) | u32::from(b1 & 0x3F);
            let ch = char::from_u32(cp).ok_or_else(|| nbt_err("invalid modified utf-8"))?;
            out.push(ch);
            i += 2;
            continue;
        }
        if b0 & 0xF0 == 0xE0 {
            let b1 = bytes
                .get(i + 1)
                .copied()
                .ok_or_else(|| nbt_err("truncated modified utf-8"))?;
            let b2 = bytes
                .get(i + 2)
                .copied()
                .ok_or_else(|| nbt_err("truncated modified utf-8"))?;
            if b1 & 0xC0 != 0x80 || b2 & 0xC0 != 0x80 {
                return Err(nbt_err("invalid modified utf-8"));
            }
            let cp =
                (u32::from(b0 & 0x0F) << 12) | (u32::from(b1 & 0x3F) << 6) | u32::from(b2 & 0x3F);
            if (0xD800..=0xDBFF).contains(&cp) {
                let lo = decode_surrogate(&bytes[i + 3..])?;
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    return Err(nbt_err("invalid modified utf-8"));
                }
                let full = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                let ch = char::from_u32(full).ok_or_else(|| nbt_err("invalid modified utf-8"))?;
                out.push(ch);
                i += 6;
            } else if (0xDC00..=0xDFFF).contains(&cp) {
                return Err(nbt_err("invalid modified utf-8"));
            } else {
                let ch = char::from_u32(cp).ok_or_else(|| nbt_err("invalid modified utf-8"))?;
                out.push(ch);
                i += 3;
            }
            continue;
        }
        return Err(nbt_err("invalid modified utf-8"));
    }
    Ok(out)
}

fn decode_surrogate(bytes: &[u8]) -> Result<u32, EngineError> {
    if bytes.len() < 3 {
        return Err(nbt_err("truncated modified utf-8"));
    }
    let b0 = bytes[0];
    let b1 = bytes[1];
    let b2 = bytes[2];
    if b0 & 0xF0 != 0xE0 || b1 & 0xC0 != 0x80 || b2 & 0xC0 != 0x80 {
        return Err(nbt_err("invalid modified utf-8"));
    }
    Ok((u32::from(b0 & 0x0F) << 12) | (u32::from(b1 & 0x3F) << 6) | u32::from(b2 & 0x3F))
}

fn nbt_err(msg: &str) -> EngineError {
    EngineError::io(
        PathBuf::from("nbt"),
        io::Error::new(io::ErrorKind::InvalidData, msg),
    )
}

#[cfg(test)]
fn encode_servers(servers: &[QuickPlayServer]) -> Vec<u8> {
    let mut buf = Vec::new();
    write_tag(&mut buf, TAG_COMPOUND);
    write_mutf8(&mut buf, "");
    write_tag(&mut buf, TAG_LIST);
    write_mutf8(&mut buf, "servers");
    write_tag(&mut buf, TAG_COMPOUND);
    write_i32(
        &mut buf,
        i32::try_from(servers.len()).expect("too many servers"),
    );
    for server in servers {
        write_named_string(&mut buf, "name", &server.name);
        write_named_string(&mut buf, "ip", &server.address);
        write_tag(&mut buf, TAG_END);
    }
    write_tag(&mut buf, TAG_END);
    buf
}

#[cfg(test)]
fn encode_level_name(name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    write_tag(&mut buf, TAG_COMPOUND);
    write_mutf8(&mut buf, "");
    write_tag(&mut buf, TAG_COMPOUND);
    write_mutf8(&mut buf, "Data");
    write_named_string(&mut buf, "LevelName", name);
    write_tag(&mut buf, TAG_END);
    write_tag(&mut buf, TAG_END);
    buf
}

#[cfg(test)]
fn write_named_string(buf: &mut Vec<u8>, name: &str, value: &str) {
    write_tag(buf, TAG_STRING);
    write_mutf8(buf, name);
    write_mutf8(buf, value);
}

#[cfg(test)]
fn write_tag(buf: &mut Vec<u8>, tag: u8) {
    buf.push(tag);
}

#[cfg(test)]
fn write_i32(buf: &mut Vec<u8>, value: i32) {
    buf.extend_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
fn write_mutf8(buf: &mut Vec<u8>, s: &str) {
    let encoded = encode_mutf8(s);
    let len = u16::try_from(encoded.len()).expect("string too long");
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(&encoded);
}

#[cfg(test)]
fn encode_mutf8(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in s.chars() {
        let cp = ch as u32;
        match cp {
            0x0000 => {
                out.push(0xC0);
                out.push(0x80);
            }
            0x0001..=0x007F => out.push(cp as u8),
            0x0080..=0x07FF => {
                out.push(0xC0 | ((cp >> 6) as u8));
                out.push(0x80 | ((cp & 0x3F) as u8));
            }
            0x0800..=0xFFFF => push_three(&mut out, cp),
            _ => {
                let u = cp - 0x10000;
                push_three(&mut out, 0xD800 + (u >> 10));
                push_three(&mut out, 0xDC00 + (u & 0x3FF));
            }
        }
    }
    out
}

#[cfg(test)]
fn push_three(out: &mut Vec<u8>, cp: u32) {
    out.push(0xE0 | ((cp >> 12) as u8));
    out.push(0x80 | (((cp >> 6) & 0x3F) as u8));
    out.push(0x80 | ((cp & 0x3F) as u8));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Loader;
    use crate::store::MemoryKeychain;
    use crate::types::CreateInstance;
    use crate::{Engine, LauncherPaths};
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    #[test]
    fn servers_round_trip() {
        let bytes = encode_servers(&[QuickPlayServer {
            name: "Hypixel".into(),
            address: "mc.hypixel.net".into(),
        }]);
        let parsed = parse_servers_dat(&bytes).unwrap();
        assert_eq!(parsed[0].name, "Hypixel");
        assert_eq!(parsed[0].address, "mc.hypixel.net");
    }

    #[test]
    fn level_name_from_compound() {
        let bytes = encode_level_name("My World");
        assert_eq!(read_level_name(&bytes).as_deref(), Some("My World"));
    }

    #[test]
    fn servers_gzip_round_trip() {
        let raw = encode_servers(&[QuickPlayServer {
            name: "Hypixel".into(),
            address: "mc.hypixel.net".into(),
        }]);
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(&gz[..2], &[0x1f, 0x8b]);
        let parsed = parse_servers_dat(&gz).unwrap();
        assert_eq!(parsed[0].name, "Hypixel");
        assert_eq!(parsed[0].address, "mc.hypixel.net");
    }

    #[test]
    fn level_name_from_gzip() {
        let raw = encode_level_name("My World");
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).unwrap();
        let gz = enc.finish().unwrap();
        assert_eq!(read_level_name(&gz).as_deref(), Some("My World"));
    }

    #[test]
    fn level_name_skips_sibling_tags() {
        let mut buf = Vec::new();
        write_tag(&mut buf, TAG_COMPOUND);
        write_mutf8(&mut buf, "");
        write_tag(&mut buf, TAG_COMPOUND);
        write_mutf8(&mut buf, "Data");
        write_tag(&mut buf, TAG_INT);
        write_mutf8(&mut buf, "DataVersion");
        write_i32(&mut buf, 3955);
        write_named_string(&mut buf, "LevelName", "Tagged");
        write_tag(&mut buf, TAG_END);
        write_tag(&mut buf, TAG_END);
        assert_eq!(read_level_name(&buf).as_deref(), Some("Tagged"));
    }

    #[tokio::test]
    async fn list_quick_play_reads_saves_and_servers() {
        let (_root, engine) = test_engine();
        let id = engine
            .create_instance(CreateInstance {
                name: "One".into(),
                minecraft_version: "1.21.1".into(),
                loader: Loader::Vanilla,
                loader_version: None,
                icon_png: None,
            })
            .await
            .unwrap();
        let mc = engine.paths.instance_minecraft("One");
        let named = mc.join("saves").join("WorldA");
        std::fs::create_dir_all(&named).unwrap();
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&encode_level_name("Pretty Name")).unwrap();
        std::fs::write(named.join("level.dat"), enc.finish().unwrap()).unwrap();
        let unlabeled = mc.join("saves").join("Raw");
        std::fs::create_dir_all(&unlabeled).unwrap();
        std::fs::write(unlabeled.join("level.dat"), b"not nbt").unwrap();
        std::fs::create_dir_all(mc.join("saves").join("Empty")).unwrap();
        std::fs::write(
            mc.join("servers.dat"),
            encode_servers(&[QuickPlayServer {
                name: "Hypixel".into(),
                address: "mc.hypixel.net".into(),
            }]),
        )
        .unwrap();

        let lists = engine.list_quick_play(id).unwrap();
        assert_eq!(
            lists.worlds,
            vec![
                QuickPlayWorld {
                    folder: "Raw".into(),
                    label: "Raw".into(),
                },
                QuickPlayWorld {
                    folder: "WorldA".into(),
                    label: "Pretty Name".into(),
                },
            ]
        );
        assert_eq!(
            lists.servers,
            vec![QuickPlayServer {
                name: "Hypixel".into(),
                address: "mc.hypixel.net".into(),
            }]
        );
    }

    #[tokio::test]
    async fn list_quick_play_missing_files_are_empty() {
        let (_root, engine) = test_engine();
        let id = engine
            .create_instance(CreateInstance {
                name: "One".into(),
                minecraft_version: "1.21.1".into(),
                loader: Loader::Vanilla,
                loader_version: None,
                icon_png: None,
            })
            .await
            .unwrap();
        let lists = engine.list_quick_play(id).unwrap();
        assert!(lists.worlds.is_empty());
        assert!(lists.servers.is_empty());
    }

    fn test_engine() -> (tempfile::TempDir, Engine) {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths, &kc).unwrap();
        (root, engine)
    }
}
