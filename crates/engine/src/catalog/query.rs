use super::provider::{CatalogProvider, ProviderId};
use super::types::{
    CatalogCategory, CatalogFile, CatalogFileFilter, CatalogPage, CatalogProject,
    CatalogProjectDetail, CatalogProjectId, CatalogQuery, ContentClass,
};
use crate::error::EngineError;
use crate::Engine;
use std::sync::Arc;

impl Engine {
    fn provider(&self, id: ProviderId) -> Result<Arc<dyn CatalogProvider>, super::CatalogError> {
        self.providers
            .lock()
            .iter()
            .find(|p| p.id() == id)
            .cloned()
            .ok_or(super::CatalogError::UnknownProvider)
    }

    pub fn add_provider(&self, p: Arc<dyn CatalogProvider>) {
        self.providers.lock().push(p);
    }

    pub async fn catalog_categories(
        &self,
        provider: ProviderId,
        class: ContentClass,
    ) -> Result<Vec<CatalogCategory>, EngineError> {
        Ok(self.provider(provider)?.categories(class).await?)
    }

    pub async fn catalog_search(
        &self,
        query: &CatalogQuery,
    ) -> Result<CatalogPage<CatalogProject>, EngineError> {
        Ok(self.provider(query.provider)?.search(query).await?)
    }

    pub async fn catalog_project(
        &self,
        provider: ProviderId,
        id: &CatalogProjectId,
    ) -> Result<CatalogProjectDetail, EngineError> {
        Ok(self.provider(provider)?.project(id).await?)
    }

    pub async fn catalog_files(
        &self,
        provider: ProviderId,
        id: &CatalogProjectId,
        filter: &CatalogFileFilter,
    ) -> Result<CatalogPage<CatalogFile>, EngineError> {
        Ok(self.provider(provider)?.files(id, filter).await?)
    }

    pub async fn catalog_file(
        &self,
        provider: ProviderId,
        project_id: &CatalogProjectId,
        file_id: &str,
    ) -> Result<CatalogFile, EngineError> {
        Ok(self.provider(provider)?.file(project_id, file_id).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::types::{
        CatalogBlob, CatalogCredentials, CatalogError, CatalogResource, CatalogSort, PackManifestSpec,
        PackOverride,
    };
    use crate::ids::Loader;
    use crate::paths::LauncherPaths;
    use crate::store::MemoryKeychain;
    use async_trait::async_trait;

    struct Fake;

    #[async_trait]
    impl CatalogProvider for Fake {
        fn id(&self) -> ProviderId {
            ProviderId::CURSEFORGE
        }
        fn label(&self) -> &'static str {
            "Fake"
        }
        fn supports(&self, class: ContentClass) -> bool {
            class == ContentClass::Mods
        }
        fn set_credentials(&self, _: CatalogCredentials) {}
        fn has_credentials(&self) -> bool {
            true
        }
        async fn categories(&self, _: ContentClass) -> Result<Vec<CatalogCategory>, CatalogError> {
            Ok(vec![])
        }
        async fn search(
            &self,
            q: &CatalogQuery,
        ) -> Result<CatalogPage<CatalogProject>, CatalogError> {
            Ok(CatalogPage {
                items: vec![CatalogProject {
                    provider: ProviderId::CURSEFORGE,
                    id: CatalogProjectId("1".into()),
                    slug: "jei".into(),
                    name: q.search.clone().unwrap_or_default(),
                    summary: String::new(),
                    authors: vec![],
                    download_count: 0,
                    logo_url: None,
                    class: ContentClass::Mods,
                }],
                index: 0,
                page_size: 20,
                total: 1,
            })
        }
        async fn project(
            &self,
            _: &CatalogProjectId,
        ) -> Result<CatalogProjectDetail, CatalogError> {
            Err(CatalogError::NotFound {
                kind: CatalogResource::Project,
                id: "-".into(),
            })
        }
        async fn files(
            &self,
            _: &CatalogProjectId,
            _: &CatalogFileFilter,
        ) -> Result<CatalogPage<CatalogFile>, CatalogError> {
            Err(CatalogError::NotFound {
                kind: CatalogResource::Project,
                id: "-".into(),
            })
        }
        async fn file(
            &self,
            _: &CatalogProjectId,
            _: &str,
        ) -> Result<CatalogFile, CatalogError> {
            Err(CatalogError::NotFound {
                kind: CatalogResource::Project,
                id: "-".into(),
            })
        }
        async fn download(&self, _: &CatalogFile) -> Result<CatalogBlob, CatalogError> {
            Err(CatalogError::NotFound {
                kind: CatalogResource::Project,
                id: "-".into(),
            })
        }
        async fn parse_pack(&self, _: &[u8]) -> Result<PackManifestSpec, CatalogError> {
            Err(CatalogError::NotFound {
                kind: CatalogResource::Project,
                id: "-".into(),
            })
        }
        fn walk_overrides(
            &self,
            _: &[u8],
            _: &mut dyn FnMut(PackOverride) -> Result<(), CatalogError>,
        ) -> Result<(), CatalogError> {
            Err(CatalogError::NotFound {
                kind: CatalogResource::Project,
                id: "-".into(),
            })
        }
        async fn resolve_required_deps(
            &self,
            _: &[CatalogFile],
            _: &str,
            _: Option<Loader>,
        ) -> Result<Vec<CatalogFile>, CatalogError> {
            Err(CatalogError::NotFound {
                kind: CatalogResource::Project,
                id: "-".into(),
            })
        }
    }

    fn test_engine() -> (tempfile::TempDir, Engine) {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths, &kc).unwrap();
        (root, engine)
    }

    #[tokio::test]
    async fn catalog_search_dispatches() {
        let (_root, engine) = test_engine();
        engine.add_provider(Arc::new(Fake));
        let page = engine
            .catalog_search(&CatalogQuery {
                class: ContentClass::Mods,
                provider: ProviderId::CURSEFORGE,
                search: Some("jei".into()),
                category_ids: vec![],
                game_version: None,
                loader: None,
                sort: CatalogSort::Popularity,
                index: 0,
                page_size: 20,
            })
            .await
            .unwrap();
        assert_eq!(page.items[0].name, "jei");
    }

    #[tokio::test]
    async fn unknown_provider_errors() {
        let (_root, engine) = test_engine();
        let err = engine
            .catalog_categories(ProviderId("modrinth"), ContentClass::Mods)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            EngineError::Catalog(CatalogError::UnknownProvider)
        ));
    }
}
