use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use async_trait::async_trait;
use kmine_curseforge::{
    CategoryFilter, ClassId, Client, File, FileFilter, FileReleaseType, Mod, ModLoaderType,
    PackZip, SearchQuery, SortField, SortOrder,
};
use kmine_engine::catalog::{
    CatalogBlob, CatalogCategory, CatalogCredentials, CatalogError, CatalogFile, CatalogFileFilter,
    CatalogPage, CatalogProject, CatalogProjectDetail, CatalogProjectId, CatalogQuery,
    CatalogRelease, CatalogResource, CatalogSort, ContentClass, PackManifestFileSpec,
    PackManifestSpec, PackOverride,
};
use kmine_engine::{CatalogProvider, Loader, ProviderId, parse_manifest_loader};

pub struct CurseForgeProvider {
    client: Mutex<Option<Client>>,
}

impl CurseForgeProvider {
    pub fn new() -> Self {
        Self {
            client: Mutex::new(None),
        }
    }

    fn client(&self) -> Result<Client, CatalogError> {
        self.client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or(CatalogError::Unavailable)
    }
}

impl Default for CurseForgeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CurseForgeProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurseForgeProvider").finish_non_exhaustive()
    }
}

#[async_trait]
impl CatalogProvider for CurseForgeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::CURSEFORGE
    }

    fn label(&self) -> &'static str {
        "CurseForge"
    }

    fn supports(&self, class: ContentClass) -> bool {
        matches!(
            class,
            ContentClass::Mods
                | ContentClass::ResourcePacks
                | ContentClass::Shaders
                | ContentClass::Modpacks
        )
    }

    fn set_credentials(&self, creds: CatalogCredentials) {
        let CatalogCredentials::ApiKey(key) = creds;
        if let Ok(client) = Client::new(key) {
            *self.client.lock().unwrap_or_else(|e| e.into_inner()) = Some(client);
        }
    }

    fn has_credentials(&self) -> bool {
        self.client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    async fn categories(&self, class: ContentClass) -> Result<Vec<CatalogCategory>, CatalogError> {
        let client = self.client()?;
        let class_id = content_to_class_id(class);
        let cats = client
            .categories(CategoryFilter::ChildrenOf(class_id))
            .await
            .map_err(map_cf_error)?;
        Ok(cats
            .into_iter()
            .map(|c| CatalogCategory {
                id: c.id.to_string(),
                name: c.name,
                class,
                parent_id: c.parent_category_id.map(|id| id.to_string()),
            })
            .collect())
    }

    async fn search(
        &self,
        query: &CatalogQuery,
    ) -> Result<CatalogPage<CatalogProject>, CatalogError> {
        let client = self.client()?;
        let class_id = content_to_class_id(query.class);
        let page_size = CatalogQuery::page_size_or_default(query.page_size);
        let (sort_field, sort_order) = map_sort(query.sort);
        let mut sq = SearchQuery::new(class_id)
            .sort(sort_field, sort_order)
            .index(query.index)
            .page_size(page_size);
        if let Some(text) = query.search.as_ref().filter(|s| !s.is_empty()) {
            sq = sq.search(text.clone());
        }
        let category_ids: Vec<u32> = query
            .category_ids
            .iter()
            .filter_map(|id| id.parse().ok())
            .collect();
        if !category_ids.is_empty() {
            sq = sq.categories(category_ids);
        }
        if let Some(version) = query.game_version.as_ref().filter(|s| !s.is_empty()) {
            sq = sq.game_version(version.clone());
        }
        if let Some(loader) = query.loader.and_then(loader_to_mod_loader) {
            sq = sq.loader(loader);
        }
        let page = client.search(&sq).await.map_err(map_cf_error)?;
        Ok(CatalogPage {
            items: page.data.iter().map(map_mod).collect(),
            index: page.pagination.index,
            page_size: page.pagination.page_size,
            total: page.pagination.total_count,
        })
    }

    async fn project(&self, id: &CatalogProjectId) -> Result<CatalogProjectDetail, CatalogError> {
        let client = self.client()?;
        let mod_id = parse_u32(&id.0, CatalogResource::Project)?;
        let m = client.get_mod(mod_id).await.map_err(map_cf_error)?;
        let description_html = match client.description(mod_id).await {
            Ok(html) => html,
            Err(kmine_curseforge::Error::NotFound { .. }) => String::new(),
            Err(err) => return Err(map_cf_error(err)),
        };
        let screenshot_urls = m
            .screenshots
            .iter()
            .filter_map(|s| s.url.clone().filter(|u| !u.is_empty()))
            .collect();
        let website_url = m.links.website_url.clone().filter(|u| !u.is_empty());
        Ok(CatalogProjectDetail {
            project: map_mod(&m),
            description_html,
            screenshot_urls,
            website_url,
        })
    }

    async fn files(
        &self,
        id: &CatalogProjectId,
        filter: &CatalogFileFilter,
    ) -> Result<CatalogPage<CatalogFile>, CatalogError> {
        let client = self.client()?;
        let mod_id = parse_u32(&id.0, CatalogResource::Project)?;
        let class = project_class(&client, mod_id).await?;
        let page_size = CatalogQuery::page_size_or_default(filter.page_size);
        let page = client
            .files(
                mod_id,
                &FileFilter {
                    game_version: filter.game_version.clone(),
                    loader: filter.loader.and_then(loader_to_mod_loader),
                    index: filter.index,
                    page_size,
                    ..FileFilter::default()
                },
            )
            .await
            .map_err(map_cf_error)?;
        Ok(CatalogPage {
            items: page.data.iter().map(|f| map_file(f, class)).collect(),
            index: page.pagination.index,
            page_size: page.pagination.page_size,
            total: page.pagination.total_count,
        })
    }

    async fn file(
        &self,
        project_id: &CatalogProjectId,
        file_id: &str,
    ) -> Result<CatalogFile, CatalogError> {
        let client = self.client()?;
        let mod_id = parse_u32(&project_id.0, CatalogResource::Project)?;
        let fid = parse_u32(file_id, CatalogResource::File)?;
        let class = project_class(&client, mod_id).await?;
        let file = client.get_file(mod_id, fid).await.map_err(map_cf_error)?;
        Ok(map_file(&file, class))
    }

    async fn download(&self, file: &CatalogFile) -> Result<CatalogBlob, CatalogError> {
        let client = self.client()?;
        let mod_id = parse_u32(&file.project_id.0, CatalogResource::Project)?;
        let file_id = parse_u32(&file.file_id, CatalogResource::File)?;
        let cf_file = client
            .get_file(mod_id, file_id)
            .await
            .map_err(map_cf_error)?;
        let downloaded = client.download(&cf_file).await.map_err(map_cf_error)?;
        Ok(CatalogBlob {
            file_name: downloaded.file_name,
            bytes: downloaded.bytes.to_vec(),
            sha1: downloaded.sha1,
        })
    }

    async fn parse_pack(&self, zip: &[u8]) -> Result<PackManifestSpec, CatalogError> {
        let client = self.client()?;
        let mut pack = PackZip::parse(zip.to_vec()).map_err(map_cf_error)?;
        let manifest = pack.manifest().map_err(map_cf_error)?;
        let primary = manifest
            .primary_loader()
            .ok_or_else(|| CatalogError::Manifest {
                message: "pack has no mod loader".into(),
            })?;
        let (loader, loader_version) =
            parse_manifest_loader(&primary.id, &manifest.minecraft.version)?;
        let resolved = client.resolve_pack(&manifest).await.map_err(map_cf_error)?;
        let required: Vec<_> = resolved.files.into_iter().filter(|f| f.required).collect();
        let ids: Vec<u32> = required.iter().map(|f| f.project_id).collect();
        let classes = classes_for_projects(&client, &ids).await?;
        let mut files = Vec::with_capacity(required.len());
        for row in required {
            let class =
                classes
                    .get(&row.project_id)
                    .copied()
                    .ok_or_else(|| CatalogError::Manifest {
                        message: format!("missing class for project {}", row.project_id),
                    })?;
            files.push(PackManifestFileSpec {
                project_id: CatalogProjectId(row.project_id.to_string()),
                file_id: row.file_id.to_string(),
                required: true,
                class,
            });
        }
        Ok(PackManifestSpec {
            name: manifest.name,
            version: manifest.version,
            minecraft_version: manifest.minecraft.version,
            loader,
            loader_version: Some(loader_version),
            files,
        })
    }

    fn walk_overrides(
        &self,
        zip: &[u8],
        visit: &mut dyn FnMut(PackOverride) -> Result<(), CatalogError>,
    ) -> Result<(), CatalogError> {
        let mut pack = PackZip::parse(zip.to_vec()).map_err(map_cf_error)?;
        pack.manifest().map_err(map_cf_error)?;
        while let Some(next) = pack.next_override() {
            let over = next.map_err(map_cf_error)?;
            visit(PackOverride {
                relative_path: over.relative_path,
                bytes: over.bytes.to_vec(),
            })?;
        }
        Ok(())
    }

    async fn resolve_required_deps(
        &self,
        roots: &[CatalogFile],
        game_version: &str,
        loader: Option<Loader>,
    ) -> Result<Vec<CatalogFile>, CatalogError> {
        let client = self.client()?;
        let mut cf_roots = Vec::with_capacity(roots.len());
        for root in roots {
            let mod_id = parse_u32(&root.project_id.0, CatalogResource::Project)?;
            let file_id = parse_u32(&root.file_id, CatalogResource::File)?;
            cf_roots.push(
                client
                    .get_file(mod_id, file_id)
                    .await
                    .map_err(map_cf_error)?,
            );
        }
        let deps = client
            .resolve_required_deps(
                &cf_roots,
                game_version,
                loader.and_then(loader_to_mod_loader),
            )
            .await
            .map_err(map_cf_error)?;
        let ids: Vec<u32> = deps.iter().map(|f| f.mod_id).collect();
        let classes = classes_for_projects(&client, &ids).await?;
        let mut out = Vec::with_capacity(deps.len());
        for file in deps {
            let class =
                classes
                    .get(&file.mod_id)
                    .copied()
                    .ok_or_else(|| CatalogError::Manifest {
                        message: format!("missing class for project {}", file.mod_id),
                    })?;
            out.push(map_file(&file, class));
        }
        Ok(out)
    }
}

fn map_mod(m: &Mod) -> CatalogProject {
    CatalogProject {
        provider: ProviderId::CURSEFORGE,
        id: CatalogProjectId(m.id.to_string()),
        slug: m.slug.clone(),
        name: m.name.clone(),
        summary: m.summary.clone(),
        authors: m.authors.iter().map(|a| a.name.clone()).collect(),
        download_count: m.download_count,
        logo_url: m
            .logo
            .as_ref()
            .and_then(|l| l.url.clone().filter(|u| !u.is_empty())),
        class: m
            .class_id
            .and_then(|id| class_id_to_content(ClassId(id)))
            .unwrap_or(ContentClass::Mods),
    }
}

fn map_file(file: &File, class: ContentClass) -> CatalogFile {
    let (game_versions, loaders) = split_game_versions(&file.game_versions);
    CatalogFile {
        provider: ProviderId::CURSEFORGE,
        project_id: CatalogProjectId(file.mod_id.to_string()),
        class,
        file_id: file.id.to_string(),
        display_name: file.display_name.clone(),
        file_name: file.file_name.clone(),
        release: map_release(file.release_type),
        game_versions,
        loaders,
        file_length: file.file_length,
        download_count: file.download_count,
        file_date: file.file_date.clone(),
    }
}

fn class_id_to_content(id: ClassId) -> Option<ContentClass> {
    if id == ClassId::MODS {
        Some(ContentClass::Mods)
    } else if id == ClassId::RESOURCE_PACKS {
        Some(ContentClass::ResourcePacks)
    } else if id == ClassId::SHADERS {
        Some(ContentClass::Shaders)
    } else if id == ClassId::MODPACKS {
        Some(ContentClass::Modpacks)
    } else {
        None
    }
}

fn content_to_class_id(class: ContentClass) -> ClassId {
    match class {
        ContentClass::Mods => ClassId::MODS,
        ContentClass::ResourcePacks => ClassId::RESOURCE_PACKS,
        ContentClass::Shaders => ClassId::SHADERS,
        ContentClass::Modpacks => ClassId::MODPACKS,
    }
}

fn loader_to_mod_loader(loader: Loader) -> Option<ModLoaderType> {
    match loader {
        Loader::Forge => Some(ModLoaderType::Forge),
        Loader::Fabric => Some(ModLoaderType::Fabric),
        Loader::NeoForge => Some(ModLoaderType::NeoForge),
        Loader::Quilt => Some(ModLoaderType::Quilt),
        Loader::Vanilla => None,
    }
}

fn loader_from_cf_tag(tag: &str) -> Option<Loader> {
    match tag.to_ascii_lowercase().as_str() {
        "forge" => Some(Loader::Forge),
        "fabric" => Some(Loader::Fabric),
        "neoforge" => Some(Loader::NeoForge),
        "quilt" => Some(Loader::Quilt),
        _ => None,
    }
}

fn split_game_versions(tags: &[String]) -> (Vec<String>, Vec<Loader>) {
    let mut versions = Vec::new();
    let mut loaders = Vec::new();
    for tag in tags {
        if let Some(loader) = loader_from_cf_tag(tag) {
            if !loaders.contains(&loader) {
                loaders.push(loader);
            }
        } else if !is_omitted_loader_tag(tag) {
            versions.push(tag.clone());
        }
    }
    (versions, loaders)
}

fn is_omitted_loader_tag(tag: &str) -> bool {
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "any" | "cauldron" | "liteloader"
    )
}

fn map_sort(sort: CatalogSort) -> (SortField, SortOrder) {
    match sort {
        CatalogSort::Popularity => (SortField::Popularity, SortOrder::Desc),
        CatalogSort::LastUpdated => (SortField::LastUpdated, SortOrder::Desc),
        CatalogSort::Downloads => (SortField::TotalDownloads, SortOrder::Desc),
        CatalogSort::Name => (SortField::Name, SortOrder::Asc),
    }
}

fn map_release(release: FileReleaseType) -> CatalogRelease {
    match release {
        FileReleaseType::Release => CatalogRelease::Release,
        FileReleaseType::Beta => CatalogRelease::Beta,
        FileReleaseType::Alpha => CatalogRelease::Alpha,
        FileReleaseType::Other(_) => CatalogRelease::Other,
    }
}

fn parse_u32(raw: &str, kind: CatalogResource) -> Result<u32, CatalogError> {
    raw.parse().map_err(|_| CatalogError::NotFound {
        kind,
        id: raw.to_string(),
    })
}

async fn project_class(client: &Client, mod_id: u32) -> Result<ContentClass, CatalogError> {
    let m = client.get_mod(mod_id).await.map_err(map_cf_error)?;
    Ok(m.class_id
        .and_then(|id| class_id_to_content(ClassId(id)))
        .unwrap_or(ContentClass::Mods))
}

async fn classes_for_projects(
    client: &Client,
    ids: &[u32],
) -> Result<HashMap<u32, ContentClass>, CatalogError> {
    let mut unique = ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let mods = client.get_mods(&unique).await.map_err(map_cf_error)?;
    Ok(mods
        .into_iter()
        .filter_map(|m| {
            let class = m.class_id.and_then(|id| class_id_to_content(ClassId(id)))?;
            Some((m.id, class))
        })
        .collect())
}

fn map_cf_error(err: kmine_curseforge::Error) -> CatalogError {
    use kmine_curseforge::{Error, ResourceKind};
    match err {
        Error::Http { url, status } => CatalogError::Http { url, status },
        Error::NotFound { kind, id } => CatalogError::NotFound {
            kind: match kind {
                ResourceKind::Mod => CatalogResource::Project,
                ResourceKind::File => CatalogResource::File,
            },
            id: id.to_string(),
        },
        Error::ChecksumMismatch {
            file_id,
            expected,
            actual,
        } => CatalogError::Checksum {
            file_id: file_id.to_string(),
            expected,
            actual,
        },
        Error::Manifest { message } | Error::Zip { message } => CatalogError::Manifest { message },
        Error::InvalidQuery { message } => CatalogError::Message(message.to_string()),
        Error::NoDownloadUrl { mod_id, file_id } => CatalogError::Message(format!(
            "no download url for project {mod_id} file {file_id}"
        )),
        Error::NoCompatibleFile {
            mod_id,
            game_version,
        } => CatalogError::Message(format!(
            "no compatible file for project {mod_id} on {game_version}"
        )),
        Error::Decode { url, message } => CatalogError::Message(format!("decode {url}: {message}")),
        Error::Builder { message } => CatalogError::Message(message.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kmine_curseforge::ClassId;
    use kmine_engine::catalog::ContentClass;

    #[test]
    fn jei_fixture_maps_to_mods() {
        // Fixture is a bare Mod object (not `{ "data": ... }`), same as kmine-curseforge tests.
        let raw = include_str!("../../crates/curseforge/tests/fixtures/mod_jei.json");
        let m: kmine_curseforge::Mod = serde_json::from_str(raw).unwrap();
        let p = map_mod(&m);
        assert_eq!(p.class, ContentClass::Mods);
        assert_eq!(p.id.0, m.id.to_string());
    }

    #[test]
    fn class_support() {
        assert!(class_id_to_content(ClassId::MODPACKS) == Some(ContentClass::Modpacks));
        assert!(class_id_to_content(ClassId::WORLDS).is_none());
    }

    #[test]
    fn file_fixture_maps_forge_loader() {
        let raw = include_str!("../../crates/curseforge/tests/fixtures/file_5754631.json");
        let f: kmine_curseforge::File = serde_json::from_str(raw).unwrap();
        let mapped = map_file(&f, ContentClass::Mods);
        assert_eq!(mapped.file_id, f.id.to_string());
        assert_eq!(mapped.project_id.0, f.mod_id.to_string());
        assert_eq!(mapped.loaders, vec![Loader::Forge]);
        assert_eq!(mapped.game_versions, vec!["1.20.1"]);
        assert_eq!(mapped.release, CatalogRelease::Release);
    }

    #[test]
    fn debug_hides_key() {
        let p = CurseForgeProvider::new();
        p.set_credentials(CatalogCredentials::ApiKey("super-secret-key-value".into()));
        assert!(p.has_credentials());
        let shown = format!("{p:?}");
        assert!(!shown.contains("super-secret-key-value"), "{shown}");
    }

    #[test]
    fn without_credentials_is_unavailable() {
        let p = CurseForgeProvider::new();
        assert!(!p.has_credentials());
        assert!(matches!(p.client(), Err(CatalogError::Unavailable)));
    }
}
