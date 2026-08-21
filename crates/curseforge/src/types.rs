use serde::Deserialize;

pub const MINECRAFT_GAME_ID: u32 = 432;
pub const DEFAULT_BASE_URL: &str = "https://api.curseforge.com";
pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 50;
pub const MAX_INDEX_PLUS_PAGE: u32 = 10_000;
pub const BATCH_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);

impl ClassId {
    pub const BUKKIT_PLUGINS: Self = Self(5);
    pub const MODS: Self = Self(6);
    pub const RESOURCE_PACKS: Self = Self(12);
    pub const WORLDS: Self = Self(17);
    pub const MODPACKS: Self = Self(4471);
    pub const CUSTOMIZATION: Self = Self(4546);
    pub const ADDONS: Self = Self(4559);
    pub const SHADERS: Self = Self(6552);
    pub const DATA_PACKS: Self = Self(6945);
}

macro_rules! cf_int_enum {
    ($name:ident, $($variant:ident => $n:expr),+ $(,)?) => {
        impl $name {
            pub fn from_u8(v: u8) -> Self {
                match v {
                    $($n => Self::$variant,)+
                    other => Self::Other(other),
                }
            }
            pub fn as_u8(self) -> u8 {
                match self {
                    $(Self::$variant => $n,)+
                    Self::Other(v) => v,
                }
            }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_u8(self.as_u8())
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Ok(Self::from_u8(u8::deserialize(d)?))
            }
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModLoaderType {
    Any,
    Forge,
    Cauldron,
    LiteLoader,
    Fabric,
    Quilt,
    NeoForge,
    Other(u8),
}
cf_int_enum!(
    ModLoaderType,
    Any => 0,
    Forge => 1,
    Cauldron => 2,
    LiteLoader => 3,
    Fabric => 4,
    Quilt => 5,
    NeoForge => 6
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortField {
    Featured,
    Popularity,
    LastUpdated,
    Name,
    Author,
    TotalDownloads,
    Category,
    GameVersion,
    EarlyAccess,
    FeaturedReleased,
    ReleasedDate,
    Rating,
    Other(u8),
}
cf_int_enum!(
    SortField,
    Featured => 1,
    Popularity => 2,
    LastUpdated => 3,
    Name => 4,
    Author => 5,
    TotalDownloads => 6,
    Category => 7,
    GameVersion => 8,
    EarlyAccess => 9,
    FeaturedReleased => 10,
    ReleasedDate => 11,
    Rating => 12
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileReleaseType {
    Release,
    Beta,
    Alpha,
    Other(u8),
}
cf_int_enum!(FileReleaseType, Release => 1, Beta => 2, Alpha => 3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileRelationType {
    EmbeddedLibrary,
    OptionalDependency,
    RequiredDependency,
    Tool,
    Incompatible,
    Include,
    Other(u8),
}
cf_int_enum!(
    FileRelationType,
    EmbeddedLibrary => 1,
    OptionalDependency => 2,
    RequiredDependency => 3,
    Tool => 4,
    Incompatible => 5,
    Include => 6
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgo {
    Sha1,
    Md5,
    Other(u8),
}
cf_int_enum!(HashAlgo, Sha1 => 1, Md5 => 2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

fn zero_as_none<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    let v = Option::<u32>::deserialize(d)?;
    Ok(v.filter(|n| *n != 0))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Page<T> {
    pub data: Vec<T>,
    pub pagination: Pagination,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            pagination: Pagination::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pagination {
    pub index: u32,
    pub page_size: u32,
    pub result_count: u32,
    pub total_count: u32,
}

impl Pagination {
    pub fn next_index(&self) -> Option<u32> {
        let next = self.index.checked_add(self.page_size)?;
        let next_end = next.checked_add(self.page_size)?;
        if next < self.total_count && next_end <= MAX_INDEX_PLUS_PAGE {
            Some(next)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Category {
    pub id: u32,
    pub game_id: u32,
    pub name: String,
    pub slug: String,
    pub url: Option<String>,
    pub icon_url: Option<String>,
    pub date_modified: Option<String>,
    pub is_class: bool,
    pub class_id: Option<u32>,
    pub parent_category_id: Option<u32>,
    pub display_index: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mod {
    pub id: u32,
    pub game_id: u32,
    pub name: String,
    pub slug: String,
    pub links: ModLinks,
    pub summary: String,
    pub status: u32,
    pub download_count: u64,
    pub is_featured: bool,
    pub primary_category_id: Option<u32>,
    pub categories: Vec<Category>,
    pub class_id: Option<u32>,
    pub authors: Vec<ModAuthor>,
    pub logo: Option<ModAsset>,
    pub screenshots: Vec<ModAsset>,
    pub main_file_id: Option<u32>,
    pub latest_files: Vec<File>,
    pub latest_files_indexes: Vec<FileIndex>,
    pub date_created: Option<String>,
    pub date_modified: Option<String>,
    pub date_released: Option<String>,
    pub allow_mod_distribution: Option<bool>,
    pub game_popularity_rank: Option<u32>,
    pub is_available: bool,
    pub thumbs_up_count: u64,
}

impl Mod {
    pub fn file_index_for(&self, mc: &str, loader: Option<ModLoaderType>) -> Option<&FileIndex> {
        self.latest_files_indexes
            .iter()
            .find(|idx| idx.matches(mc, loader))
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModLinks {
    pub website_url: Option<String>,
    pub wiki_url: Option<String>,
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModAuthor {
    pub id: u32,
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModAsset {
    pub id: u32,
    pub mod_id: Option<u32>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub thumbnail_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileIndex {
    pub game_version: String,
    pub file_id: u32,
    pub filename: String,
    pub release_type: FileReleaseType,
    pub game_version_type_id: Option<u32>,
    pub mod_loader: Option<ModLoaderType>,
}

impl Default for FileIndex {
    fn default() -> Self {
        Self {
            game_version: String::new(),
            file_id: 0,
            filename: String::new(),
            release_type: FileReleaseType::Other(0),
            game_version_type_id: None,
            mod_loader: None,
        }
    }
}

impl FileIndex {
    pub fn matches(&self, mc: &str, loader: Option<ModLoaderType>) -> bool {
        if self.game_version != mc {
            return false;
        }
        match loader {
            None | Some(ModLoaderType::Any) => true,
            Some(want) => match self.mod_loader {
                None | Some(ModLoaderType::Any) => true,
                Some(have) => have == want,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct File {
    pub id: u32,
    pub game_id: u32,
    pub mod_id: u32,
    pub is_available: bool,
    pub display_name: String,
    pub file_name: String,
    pub release_type: FileReleaseType,
    pub file_status: u32,
    pub hashes: Vec<FileHash>,
    pub file_date: Option<String>,
    pub file_length: u64,
    pub download_count: u64,
    pub download_url: Option<String>,
    pub game_versions: Vec<String>,
    pub sortable_game_versions: Vec<SortableGameVersion>,
    pub dependencies: Vec<FileDependency>,
    #[serde(default, deserialize_with = "zero_as_none")]
    pub alternate_file_id: Option<u32>,
    pub is_server_pack: bool,
    #[serde(default, deserialize_with = "zero_as_none")]
    pub server_pack_file_id: Option<u32>,
    pub is_early_access_content: bool,
    pub file_fingerprint: u32,
    pub modules: Vec<FileModule>,
}

impl Default for File {
    fn default() -> Self {
        Self {
            id: 0,
            game_id: 0,
            mod_id: 0,
            is_available: false,
            display_name: String::new(),
            file_name: String::new(),
            release_type: FileReleaseType::Other(0),
            file_status: 0,
            hashes: Vec::new(),
            file_date: None,
            file_length: 0,
            download_count: 0,
            download_url: None,
            game_versions: Vec::new(),
            sortable_game_versions: Vec::new(),
            dependencies: Vec::new(),
            alternate_file_id: None,
            is_server_pack: false,
            server_pack_file_id: None,
            is_early_access_content: false,
            file_fingerprint: 0,
            modules: Vec::new(),
        }
    }
}

impl File {
    pub fn sha1(&self) -> Option<&str> {
        self.hashes
            .iter()
            .find(|h| h.algo == HashAlgo::Sha1)
            .map(|h| h.value.as_str())
    }

    pub fn md5(&self) -> Option<&str> {
        self.hashes
            .iter()
            .find(|h| h.algo == HashAlgo::Md5)
            .map(|h| h.value.as_str())
    }

    pub fn is_approved(&self) -> bool {
        self.is_available && self.file_status == 4
    }

    pub fn required_mod_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.dependencies.iter().filter_map(|d| {
            (d.relation_type == FileRelationType::RequiredDependency).then_some(d.mod_id)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileHash {
    pub algo: HashAlgo,
    pub value: String,
}

impl Default for FileHash {
    fn default() -> Self {
        Self {
            algo: HashAlgo::Other(0),
            value: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileDependency {
    pub mod_id: u32,
    pub relation_type: FileRelationType,
}

impl Default for FileDependency {
    fn default() -> Self {
        Self {
            mod_id: 0,
            relation_type: FileRelationType::Other(0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileModule {
    pub name: String,
    pub fingerprint: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SortableGameVersion {
    pub game_version_name: Option<String>,
    pub game_version_padded: Option<String>,
    pub game_version: Option<String>,
    pub game_version_release_date: Option<String>,
    pub game_version_type_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MinecraftVersion {
    pub id: Option<u32>,
    pub game_version_id: Option<u32>,
    pub version_string: String,
    pub jar_download_url: Option<String>,
    pub json_download_url: Option<String>,
    pub approved: Option<bool>,
    pub date_modified: Option<String>,
    pub game_version_type_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModLoaderIndexEntry {
    pub name: String,
    pub game_version: String,
    pub latest: bool,
    pub recommended: bool,
    pub date_modified: Option<String>,
    #[serde(rename = "type")]
    pub loader_type: Option<ModLoaderType>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModLoaderInfo {
    pub name: String,
    pub game_version: String,
    pub latest: bool,
    pub recommended: bool,
    pub download_url: Option<String>,
    pub filename: Option<String>,
    pub install_method: Option<i32>,
    pub libraries_install_location: Option<String>,
    pub version_json: Option<serde_json::Value>,
    pub install_profile_json: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_mod() -> Mod {
        serde_json::from_str(include_str!("../tests/fixtures/mod_jei.json")).unwrap()
    }

    fn load_file() -> File {
        serde_json::from_str(include_str!("../tests/fixtures/file_5754631.json")).unwrap()
    }

    #[test]
    fn jei_mod_fixture() {
        let m = load_mod();
        assert_eq!(m.id, 238222);
        assert_eq!(m.slug, "jei");
        assert_eq!(m.class_id, Some(6));
        assert_eq!(
            m.latest_files_indexes[0].mod_loader,
            Some(ModLoaderType::Forge)
        );
    }

    #[test]
    fn file_5754631_fixture() {
        let f = load_file();
        assert_eq!(f.file_name, "oreexcavation-1.13.174.jar");
        assert_eq!(f.file_fingerprint, 3871571640);
        assert_eq!(f.sha1(), Some("19b1540f5e69fe6d04d174915e834bb614bf51ce"));
        assert_eq!(f.md5(), Some("1e87b83ed930e864de2a3150255f30bf"));
        assert_eq!(f.required_mod_ids().collect::<Vec<_>>(), vec![123456]);
        assert!(f.is_approved());
    }

    #[test]
    fn unknown_loader_is_other() {
        let v: ModLoaderType = serde_json::from_str("99").unwrap();
        assert_eq!(v, ModLoaderType::Other(99));
        assert_eq!(serde_json::to_string(&v).unwrap(), "99");
    }

    #[test]
    fn pagination_next_index() {
        let p = Pagination {
            index: 0,
            page_size: 20,
            result_count: 20,
            total_count: 55,
        };
        assert_eq!(p.next_index(), Some(20));
        let end = Pagination {
            index: 40,
            page_size: 20,
            result_count: 15,
            total_count: 55,
        };
        assert_eq!(end.next_index(), None);
        let cap = Pagination {
            index: 9960,
            page_size: 50,
            result_count: 50,
            total_count: 20000,
        };
        assert_eq!(cap.next_index(), None);
    }

    #[test]
    fn file_index_matches_loader_rules() {
        let row = FileIndex {
            game_version: "1.20.1".into(),
            file_id: 1,
            filename: "a.jar".into(),
            release_type: FileReleaseType::Release,
            game_version_type_id: Some(1),
            mod_loader: Some(ModLoaderType::Forge),
        };
        assert!(row.matches("1.20.1", None));
        assert!(row.matches("1.20.1", Some(ModLoaderType::Any)));
        assert!(row.matches("1.20.1", Some(ModLoaderType::Forge)));
        assert!(!row.matches("1.20.1", Some(ModLoaderType::Fabric)));
        assert!(!row.matches("1.21.1", Some(ModLoaderType::Forge)));
        let any_row = FileIndex {
            mod_loader: Some(ModLoaderType::Any),
            ..row.clone()
        };
        assert!(any_row.matches("1.20.1", Some(ModLoaderType::Fabric)));
        let none_row = FileIndex {
            mod_loader: None,
            ..row
        };
        assert!(none_row.matches("1.20.1", Some(ModLoaderType::Forge)));
    }

    #[test]
    fn file_index_for_picks_first_match() {
        let m = load_mod();
        let idx = m
            .file_index_for("1.20.1", Some(ModLoaderType::Forge))
            .unwrap();
        assert_eq!(idx.file_id, 5700000);
        assert!(
            m.file_index_for("1.20.1", Some(ModLoaderType::Fabric))
                .is_none()
        );
    }

    #[test]
    fn class_id_constants() {
        assert_eq!(ClassId::MODS.0, 6);
        assert_eq!(ClassId::MODPACKS.0, 4471);
        assert_eq!(ClassId::RESOURCE_PACKS.0, 12);
        assert_eq!(ClassId::SHADERS.0, 6552);
        assert_eq!(MINECRAFT_GAME_ID, 432);
        assert_eq!(DEFAULT_PAGE_SIZE, 20);
        assert_eq!(MAX_PAGE_SIZE, 50);
        assert_eq!(BATCH_SIZE, 100);
    }
}
