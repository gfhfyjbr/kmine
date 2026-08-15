use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(pub Uuid);

impl InstanceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn as_hyphenated(&self) -> String {
        self.0.as_hyphenated().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub Uuid);

impl AccountId {
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    pub fn as_hyphenated(&self) -> String {
        self.0.as_hyphenated().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}

impl Loader {
    pub fn as_str(self) -> &'static str {
        match self {
            Loader::Vanilla => "vanilla",
            Loader::Fabric => "fabric",
            Loader::Forge => "forge",
            Loader::NeoForge => "neoforge",
            Loader::Quilt => "quilt",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Loader;

    #[test]
    fn loader_serde_lowercase() {
        assert_eq!(
            serde_json::from_str::<Loader>("\"vanilla\"").unwrap(),
            Loader::Vanilla
        );
        assert_eq!(
            serde_json::from_str::<Loader>("\"fabric\"").unwrap(),
            Loader::Fabric
        );
        assert_eq!(
            serde_json::from_str::<Loader>("\"forge\"").unwrap(),
            Loader::Forge
        );
        assert_eq!(serde_json::to_string(&Loader::Forge).unwrap(), "\"forge\"");
        assert_eq!(
            serde_json::from_str::<Loader>("\"neoforge\"").unwrap(),
            Loader::NeoForge
        );
        assert_eq!(
            serde_json::from_str::<Loader>("\"quilt\"").unwrap(),
            Loader::Quilt
        );
        assert_eq!(
            serde_json::to_string(&Loader::NeoForge).unwrap(),
            "\"neoforge\""
        );
        assert_eq!(serde_json::to_string(&Loader::Quilt).unwrap(), "\"quilt\"");
    }
}
