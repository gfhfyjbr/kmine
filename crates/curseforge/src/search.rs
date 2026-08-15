use crate::Error;
use crate::types::{
    ClassId, DEFAULT_PAGE_SIZE, MAX_INDEX_PLUS_PAGE, MAX_PAGE_SIZE, ModLoaderType, SortField,
    SortOrder,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryFilter {
    All,
    ClassesOnly,
    ChildrenOf(ClassId),
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub(crate) class: ClassId,
    pub(crate) search: Option<String>,
    pub(crate) categories: Vec<u32>,
    pub(crate) game_versions: Vec<String>,
    pub(crate) loaders: Vec<ModLoaderType>,
    pub(crate) sort_field: SortField,
    pub(crate) sort_order: SortOrder,
    pub(crate) slug: Option<String>,
    pub(crate) author_id: Option<u32>,
    pub(crate) game_version_type_id: Option<u32>,
    pub(crate) index: u32,
    pub(crate) page_size: u32,
}

impl SearchQuery {
    pub fn new(class: ClassId) -> Self {
        Self {
            class,
            search: None,
            categories: Vec::new(),
            game_versions: Vec::new(),
            loaders: Vec::new(),
            sort_field: SortField::Popularity,
            sort_order: SortOrder::Desc,
            slug: None,
            author_id: None,
            game_version_type_id: None,
            index: 0,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }

    pub fn search(mut self, text: impl Into<String>) -> Self {
        self.search = Some(text.into());
        self
    }

    pub fn categories(mut self, ids: impl Into<Vec<u32>>) -> Self {
        self.categories = ids.into();
        self
    }

    pub fn category(mut self, id: u32) -> Self {
        self.categories.push(id);
        self
    }

    pub fn game_versions(mut self, versions: impl Into<Vec<String>>) -> Self {
        self.game_versions = versions.into();
        self
    }

    pub fn game_version(mut self, v: impl Into<String>) -> Self {
        self.game_versions.push(v.into());
        self
    }

    pub fn loaders(mut self, loaders: impl Into<Vec<ModLoaderType>>) -> Self {
        self.loaders = loaders.into();
        self
    }

    pub fn loader(mut self, loader: ModLoaderType) -> Self {
        self.loaders.push(loader);
        self
    }

    pub fn sort(mut self, field: SortField, order: SortOrder) -> Self {
        self.sort_field = field;
        self.sort_order = order;
        self
    }

    pub fn slug(mut self, slug: impl Into<String>) -> Self {
        self.slug = Some(slug.into());
        self
    }

    pub fn author_id(mut self, id: u32) -> Self {
        self.author_id = Some(id);
        self
    }

    pub fn game_version_type_id(mut self, id: u32) -> Self {
        self.game_version_type_id = Some(id);
        self
    }

    pub fn index(mut self, index: u32) -> Self {
        self.index = index;
        self
    }

    pub fn page_size(mut self, page_size: u32) -> Self {
        self.page_size = page_size;
        self
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        if !(1..=MAX_PAGE_SIZE).contains(&self.page_size) {
            return Err(Error::InvalidQuery {
                message: "pageSize must be 1..=50",
            });
        }
        if self.index.saturating_add(self.page_size) > MAX_INDEX_PLUS_PAGE {
            return Err(Error::InvalidQuery {
                message: "index + pageSize exceeds 10000",
            });
        }
        if self.categories.len() > 10 {
            return Err(Error::InvalidQuery {
                message: "categoryIds max 10",
            });
        }
        if self.game_versions.len() > 4 {
            return Err(Error::InvalidQuery {
                message: "gameVersions max 4",
            });
        }
        if self.loaders.len() > 5 {
            return Err(Error::InvalidQuery {
                message: "modLoaderTypes max 5",
            });
        }
        Ok(())
    }
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self::new(ClassId::MODS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFilter {
    pub game_version: Option<String>,
    pub game_version_type_id: Option<u32>,
    pub loader: Option<ModLoaderType>,
    pub client_compatible: Option<bool>,
    pub index: u32,
    pub page_size: u32,
}

impl Default for FileFilter {
    fn default() -> Self {
        Self {
            game_version: None,
            game_version_type_id: None,
            loader: None,
            client_compatible: None,
            index: 0,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
}
