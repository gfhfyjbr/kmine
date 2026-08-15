use crate::Error;
use serde::Deserialize;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
