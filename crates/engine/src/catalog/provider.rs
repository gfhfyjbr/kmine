use super::types::{
    CatalogBlob, CatalogCategory, CatalogCredentials, CatalogError, CatalogFile, CatalogFileFilter,
    CatalogPage, CatalogProject, CatalogProjectDetail, CatalogProjectId, CatalogQuery,
    ContentClass, PackManifestSpec, PackOverride,
};
use crate::ids::Loader;
use async_trait::async_trait;

/// Stable catalog provider identity (e.g. `"curseforge"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderId(pub &'static str);

impl ProviderId {
    pub const CURSEFORGE: Self = Self("curseforge");
}

#[async_trait]
pub trait CatalogProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn label(&self) -> &'static str;
    fn supports(&self, class: ContentClass) -> bool;

    fn set_credentials(&self, creds: CatalogCredentials);
    fn has_credentials(&self) -> bool;

    async fn categories(&self, class: ContentClass) -> Result<Vec<CatalogCategory>, CatalogError>;
    async fn search(
        &self,
        query: &CatalogQuery,
    ) -> Result<CatalogPage<CatalogProject>, CatalogError>;
    async fn project(&self, id: &CatalogProjectId) -> Result<CatalogProjectDetail, CatalogError>;
    async fn files(
        &self,
        id: &CatalogProjectId,
        filter: &CatalogFileFilter,
    ) -> Result<CatalogPage<CatalogFile>, CatalogError>;
    async fn file(
        &self,
        project_id: &CatalogProjectId,
        file_id: &str,
    ) -> Result<CatalogFile, CatalogError>;
    async fn download(&self, file: &CatalogFile) -> Result<CatalogBlob, CatalogError>;

    async fn parse_pack(&self, zip: &[u8]) -> Result<PackManifestSpec, CatalogError>;
    fn walk_overrides(
        &self,
        zip: &[u8],
        visit: &mut dyn FnMut(PackOverride) -> Result<(), CatalogError>,
    ) -> Result<(), CatalogError>;

    async fn resolve_required_deps(
        &self,
        roots: &[CatalogFile],
        game_version: &str,
        loader: Option<Loader>,
    ) -> Result<Vec<CatalogFile>, CatalogError>;
}
