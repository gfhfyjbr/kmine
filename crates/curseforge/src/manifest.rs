use crate::Error;
use crate::types::File;
use bytes::Bytes;
use serde::Deserialize;
use std::io::{Cursor, Read};
use zip::ZipArchive;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub minecraft: ManifestMinecraft,
    pub manifest_type: String,
    pub manifest_version: u32,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
    #[serde(default)]
    pub files: Vec<ManifestFile>,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestMinecraft {
    pub version: String,
    #[serde(default)]
    pub mod_loaders: Vec<ManifestLoader>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ManifestLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ManifestFile {
    #[serde(rename = "projectID")]
    pub project_id: u32,
    #[serde(rename = "fileID")]
    pub file_id: u32,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPack {
    pub manifest: Manifest,
    pub files: Vec<ResolvedPackFile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPackFile {
    pub project_id: u32,
    pub file_id: u32,
    pub required: bool,
    pub file: File,
}

impl Manifest {
    pub fn parse(json: &[u8]) -> Result<Self, Error> {
        let parsed: Manifest = serde_json::from_slice(json).map_err(|err| Error::Manifest {
            message: err.to_string(),
        })?;
        if parsed.manifest_type != "minecraftModpack" {
            return Err(Error::Manifest {
                message: format!("unsupported manifestType {}", parsed.manifest_type),
            });
        }
        if parsed.minecraft.version.is_empty() {
            return Err(Error::Manifest {
                message: "minecraft.version is empty".into(),
            });
        }
        Ok(parsed)
    }

    pub fn primary_loader(&self) -> Option<&ManifestLoader> {
        self.minecraft
            .mod_loaders
            .iter()
            .find(|l| l.primary)
            .or_else(|| self.minecraft.mod_loaders.first())
    }
}

pub struct PackOverride {
    pub relative_path: String,
    pub bytes: Bytes,
}

pub struct PackZip {
    archive: ZipArchive<Cursor<Bytes>>,
    prefix: Option<String>,
    overrides: String,
    next_index: usize,
    ready: bool,
}

impl PackZip {
    pub fn parse(bytes: impl Into<Bytes>) -> Result<Self, Error> {
        let bytes = bytes.into();
        let archive = ZipArchive::new(Cursor::new(bytes)).map_err(|err| Error::Zip {
            message: err.to_string(),
        })?;
        Ok(Self {
            archive,
            prefix: None,
            overrides: "overrides".into(),
            next_index: 0,
            ready: false,
        })
    }

    pub fn manifest(&mut self) -> Result<Manifest, Error> {
        let (name, prefix) = find_manifest_name(&self.archive)?;
        let mut file = self.archive.by_name(&name).map_err(|err| Error::Zip {
            message: err.to_string(),
        })?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|err| Error::Zip {
            message: err.to_string(),
        })?;
        drop(file);
        let parsed = Manifest::parse(&buf)?;
        self.prefix = prefix;
        self.overrides = parsed.overrides.clone();
        self.ready = true;
        self.next_index = 0;
        Ok(parsed)
    }

    pub fn next_override(&mut self) -> Option<Result<PackOverride, Error>> {
        if !self.ready {
            return Some(Err(Error::Manifest {
                message: "manifest() not called".into(),
            }));
        }
        let prefix = self.prefix.clone().unwrap_or_default();
        let root = if prefix.is_empty() {
            format!("{}/", self.overrides.trim_end_matches('/'))
        } else {
            format!(
                "{}/{}/",
                prefix.trim_end_matches('/'),
                self.overrides.trim_end_matches('/')
            )
        };
        while self.next_index < self.archive.len() {
            let i = self.next_index;
            self.next_index += 1;
            let mut file = match self.archive.by_index(i) {
                Ok(f) => f,
                Err(err) => return Some(Err(Error::Zip { message: err.to_string() })),
            };
            if file.is_dir() {
                continue;
            }
            let name = file.name().replace('\\', "/");
            let Some(rel) = name.strip_prefix(&root) else {
                continue;
            };
            if rel.is_empty() {
                continue;
            }
            let mut buf = Vec::new();
            if let Err(err) = file.read_to_end(&mut buf) {
                return Some(Err(Error::Zip {
                    message: err.to_string(),
                }));
            }
            return Some(Ok(PackOverride {
                relative_path: rel.to_string(),
                bytes: Bytes::from(buf),
            }));
        }
        None
    }
}

fn find_manifest_name<R: std::io::Read + std::io::Seek>(
    archive: &ZipArchive<R>,
) -> Result<(String, Option<String>), Error> {
    let names: Vec<String> = archive.file_names().map(|s| s.replace('\\', "/")).collect();
    if names.iter().any(|n| n == "manifest.json") {
        return Ok(("manifest.json".into(), None));
    }
    let mut tops = std::collections::BTreeSet::new();
    for n in &names {
        let top = n.split('/').next().unwrap_or(n);
        if !top.is_empty() {
            tops.insert(top.to_string());
        }
    }
    if tops.len() == 1 {
        let top = tops.iter().next().unwrap();
        let candidate = format!("{top}/manifest.json");
        if names.iter().any(|n| n == &candidate) {
            return Ok((candidate, Some(top.clone())));
        }
    }
    Err(Error::Manifest {
        message: "manifest.json not found".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_with(entries: &[(&str, &[u8])]) -> bytes::Bytes {
        use std::io::{Cursor, Write};
        let mut buf = Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        bytes::Bytes::from(buf.into_inner())
    }

    #[test]
    fn parse_skyfactory_shaped() {
        let m = Manifest::parse(include_bytes!("../tests/fixtures/manifest_sf5.json")).unwrap();
        assert_eq!(m.minecraft.version, "1.20.1");
        assert_eq!(m.primary_loader().unwrap().id, "forge-47.4.0");
        assert_eq!(m.files.len(), 1);
        assert_eq!(m.files[0].project_id, 430225);
        assert_eq!(m.files[0].file_id, 5707939);
        assert!(m.files[0].required);
        assert_eq!(m.overrides, "overrides");
    }

    #[test]
    fn rejects_wrong_type() {
        let err = Manifest::parse(br#"{"minecraft":{"version":"1.20.1","modLoaders":[]},"manifestType":"other","manifestVersion":1,"name":"x","version":"1","files":[]}"#).unwrap_err();
        assert!(matches!(err, crate::Error::Manifest { .. }));
    }

    #[test]
    fn rejects_empty_mc_version() {
        let err = Manifest::parse(br#"{"minecraft":{"version":"","modLoaders":[]},"manifestType":"minecraftModpack","manifestVersion":1,"name":"x","version":"1","files":[]}"#).unwrap_err();
        assert!(matches!(err, crate::Error::Manifest { .. }));
    }

    #[test]
    fn required_defaults_true_overrides_default() {
        let m = Manifest::parse(br#"{"minecraft":{"version":"1.20.1","modLoaders":[]},"manifestType":"minecraftModpack","manifestVersion":1,"name":"x","version":"1","files":[{"projectID":1,"fileID":2}]}"#).unwrap();
        assert!(m.files[0].required);
        assert_eq!(m.overrides, "overrides");
    }

    #[test]
    fn pack_zip_root_manifest_and_override() {
        let json = include_bytes!("../tests/fixtures/manifest_sf5.json");
        let bytes = zip_with(&[
            ("manifest.json", json.as_slice()),
            ("overrides/config/a.txt", b"hi"),
        ]);
        let mut pack = PackZip::parse(bytes).unwrap();
        let m = pack.manifest().unwrap();
        assert_eq!(m.name, "SkyFactory 5");
        let over = pack.next_override().unwrap().unwrap();
        assert_eq!(over.relative_path, "config/a.txt");
        assert_eq!(&over.bytes[..], b"hi");
        assert!(pack.next_override().is_none());
    }

    #[test]
    fn pack_zip_wrapped_folder() {
        let json = include_bytes!("../tests/fixtures/manifest_sf5.json");
        let bytes = zip_with(&[
            ("SF5/manifest.json", json.as_slice()),
            ("SF5/overrides/config/a.txt", b"hi"),
        ]);
        let mut pack = PackZip::parse(bytes).unwrap();
        assert_eq!(pack.manifest().unwrap().name, "SkyFactory 5");
        let over = pack.next_override().unwrap().unwrap();
        assert_eq!(over.relative_path, "config/a.txt");
    }

    #[test]
    fn pack_zip_two_roots_without_manifest_errors() {
        let bytes = zip_with(&[("a/foo.txt", b"1"), ("b/bar.txt", b"2")]);
        let mut pack = PackZip::parse(bytes).unwrap();
        assert!(matches!(pack.manifest().unwrap_err(), crate::Error::Manifest { .. }));
    }
}
