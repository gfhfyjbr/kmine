use super::cache;
use super::provider::{CatalogProvider, ProviderId};
use super::types::{CatalogBlob, CatalogError, CatalogFile, CatalogProjectId};
use crate::error::EngineError;
use crate::http::HttpFiles;
use crate::ids::InstanceId;
use crate::paths::{self, LauncherPaths};
use crate::types::{ContentFolder, CreateInstance, ProgressSink};
use crate::{Engine, Event};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

/// Engine-wide install flag. The parking_lot guard is not held across `.await`
/// (it is `!Send`); `try_lock` only serializes the acquire/check/set.
struct InstallLock<'a>(&'a parking_lot::Mutex<bool>);

impl<'a> InstallLock<'a> {
    fn acquire(lock: &'a parking_lot::Mutex<bool>) -> Result<Self, EngineError> {
        let mut guard = lock.try_lock().ok_or(EngineError::InstanceBusy)?;
        if *guard {
            return Err(EngineError::InstanceBusy);
        }
        *guard = true;
        Ok(Self(lock))
    }
}

impl Drop for InstallLock<'_> {
    fn drop(&mut self) {
        *self.0.lock() = false;
    }
}

impl Engine {
    pub async fn install_pack(
        &self,
        provider: ProviderId,
        project_id: &CatalogProjectId,
        file_id: &str,
        name_override: Option<String>,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
    ) -> Result<InstanceId, EngineError> {
        let _installing = InstallLock::acquire(&self.installing)?;
        self.install_pack_locked(
            provider,
            project_id,
            file_id,
            name_override,
            progress,
            cancel,
        )
        .await
    }

    pub async fn cache_remote_image(&self, url: &str) -> Result<PathBuf, EngineError> {
        let hash = cache::sha1_hex(url.as_bytes());
        let dir = &self.paths.cache_catalog_images;
        for ext in [".png", ".jpg", ".webp", ".img"] {
            let path = dir.join(format!("{hash}{ext}"));
            if path.is_file() {
                return Ok(path);
            }
        }

        let http = HttpFiles::new()?;
        let response = http.client.get(url).send().await.map_err(|err| {
            EngineError::io(PathBuf::from(url), std::io::Error::other(err.to_string()))
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(EngineError::Http {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let bytes = response.bytes().await.map_err(|err| {
            EngineError::io(PathBuf::from(url), std::io::Error::other(err.to_string()))
        })?;
        let ext = image_ext(content_type.as_deref(), url);
        std::fs::create_dir_all(dir).map_err(|e| EngineError::io(dir, e))?;
        let dest = dir.join(format!("{hash}{ext}"));
        std::fs::write(&dest, &bytes).map_err(|e| EngineError::io(&dest, e))?;
        Ok(dest)
    }

    async fn install_pack_locked(
        &self,
        provider: ProviderId,
        project_id: &CatalogProjectId,
        file_id: &str,
        name_override: Option<String>,
        progress: &dyn ProgressSink,
        cancel: &CancellationToken,
    ) -> Result<InstanceId, EngineError> {
        check_cancel(cancel)?;
        let catalog = self.provider(provider)?;

        progress.set("Pack zip", 0, 1);
        let pack_file = catalog.file(project_id, file_id).await?;
        let (_pack_path, pack_blob) =
            fetch_blob(&*catalog, &self.paths, provider, &pack_file, cancel).await?;
        progress.set("Pack zip", 1, 1);

        check_cancel(cancel)?;
        let manifest = catalog.parse_pack(&pack_blob.bytes).await?;
        check_cancel(cancel)?;

        let icon_png = match catalog.project(project_id).await {
            Ok(detail) => match detail.project.logo_url {
                Some(url) => match self.cache_remote_image(&url).await {
                    Ok(path) => std::fs::read(path).ok(),
                    Err(_) => None,
                },
                None => None,
            },
            Err(_) => None,
        };

        let id = self
            .create_instance(CreateInstance {
                name: name_override.unwrap_or(manifest.name),
                minecraft_version: manifest.minecraft_version.clone(),
                loader: manifest.loader,
                loader_version: manifest.loader_version.clone(),
                icon_png,
            })
            .await?;

        let after = async {
            check_cancel(cancel)?;
            let row = self
                .get_instance(id)?
                .ok_or_else(|| crate::instance_not_found(&self.paths))?;
            let mc = self.paths.instance_minecraft(&row.slug);

            let required: Vec<_> = manifest.files.iter().filter(|f| f.required).collect();
            let n = required.len();
            for (idx, spec) in required.iter().enumerate() {
                check_cancel(cancel)?;
                let i = idx + 1;
                progress.set(&format!("Files {i}/{n}"), i as u64, n as u64);
                let Some(folder) = spec.class.dest_folder() else {
                    return Err(CatalogError::Message(format!(
                        "required pack file {} has no dest folder",
                        spec.file_id
                    ))
                    .into());
                };
                let file = catalog.file(&spec.project_id, &spec.file_id).await?;
                let (cached, blob) =
                    fetch_blob(&*catalog, &self.paths, provider, &file, cancel).await?;
                check_cancel(cancel)?;
                copy_cached_file(&mc, folder, &blob.file_name, &cached)?;
            }

            check_cancel(cancel)?;
            write_overrides(&*catalog, &pack_blob.bytes, &mc, progress, cancel)?;
            check_cancel(cancel)?;
            Ok(())
        };

        if let Err(err) = after.await {
            let _ = self.delete_instance(id).await;
            return Err(prefer_cancelled(err, cancel));
        }

        self.emit(Event::InstancesChanged);
        Ok(id)
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), EngineError> {
    if cancel.is_cancelled() {
        Err(EngineError::Cancelled)
    } else {
        Ok(())
    }
}

fn prefer_cancelled(err: EngineError, cancel: &CancellationToken) -> EngineError {
    if cancel.is_cancelled() {
        EngineError::Cancelled
    } else {
        err
    }
}

async fn fetch_blob(
    catalog: &dyn CatalogProvider,
    paths: &LauncherPaths,
    provider: ProviderId,
    file: &CatalogFile,
    cancel: &CancellationToken,
) -> Result<(PathBuf, CatalogBlob), EngineError> {
    check_cancel(cancel)?;
    let dest = cache::blob_path(paths, provider, &file.file_id, &file.file_name);
    if dest.is_file() {
        let existing = std::fs::read(&dest).map_err(|e| CatalogError::Message(e.to_string()))?;
        if !existing.is_empty() {
            return Ok((
                dest,
                CatalogBlob {
                    file_name: file.file_name.clone(),
                    bytes: existing,
                    sha1: None,
                },
            ));
        }
    }
    let blob = catalog.download(file).await?;
    check_cancel(cancel)?;
    let dest = cache::blob_path(paths, provider, &file.file_id, &blob.file_name);
    cache::put_blob(&dest, &blob)?;
    Ok((dest, blob))
}

fn copy_cached_file(
    instance_minecraft: &Path,
    folder: ContentFolder,
    file_name: &str,
    src: &Path,
) -> Result<(), EngineError> {
    let dest_dir = paths::safe_join(instance_minecraft, folder.dir_name())?;
    std::fs::create_dir_all(&dest_dir).map_err(|e| EngineError::io(&dest_dir, e))?;
    if let (Ok(parent), Ok(mc)) = (dest_dir.canonicalize(), instance_minecraft.canonicalize())
        && !parent.starts_with(&mc)
    {
        return Err(EngineError::io(
            dest_dir,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "dest escapes instance"),
        ));
    }
    let dest = paths::safe_join(&dest_dir, file_name)?;
    std::fs::copy(src, &dest).map_err(|e| EngineError::io(&dest, e))?;
    Ok(())
}

fn write_overrides(
    catalog: &dyn CatalogProvider,
    pack_zip: &[u8],
    instance_minecraft: &Path,
    progress: &dyn ProgressSink,
    cancel: &CancellationToken,
) -> Result<(), EngineError> {
    let mut i = 0u64;
    let walk = catalog.walk_overrides(pack_zip, &mut |ovr| {
        if cancel.is_cancelled() {
            return Err(CatalogError::Message("cancelled".into()));
        }
        i += 1;
        progress.set(&format!("Overrides {i}/0"), i, 0);
        let dest = paths::safe_join(instance_minecraft, &ovr.relative_path)
            .map_err(|_| CatalogError::Message("unsafe override path".into()))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CatalogError::Message(e.to_string()))?;
        }
        std::fs::write(&dest, &ovr.bytes).map_err(|e| CatalogError::Message(e.to_string()))?;
        Ok(())
    });
    if cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }
    walk?;
    Ok(())
}

fn image_ext(content_type: Option<&str>, url: &str) -> &'static str {
    if let Some(ct) = content_type {
        let ct = ct
            .split(';')
            .next()
            .unwrap_or(ct)
            .trim()
            .to_ascii_lowercase();
        match ct.as_str() {
            "image/png" => return ".png",
            "image/jpeg" | "image/jpg" => return ".jpg",
            "image/webp" => return ".webp",
            _ => {}
        }
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        ".png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        ".jpg"
    } else if lower.ends_with(".webp") {
        ".webp"
    } else {
        ".img"
    }
}

#[cfg(test)]
mod tests {
    use super::super::provider::{CatalogProvider, ProviderId};
    use super::super::types::{
        CatalogBlob, CatalogCategory, CatalogCredentials, CatalogError, CatalogFile,
        CatalogFileFilter, CatalogPage, CatalogProject, CatalogProjectDetail, CatalogProjectId,
        CatalogQuery, CatalogRelease, CatalogResource, ContentClass, PackManifestFileSpec,
        PackManifestSpec, PackOverride,
    };
    use crate::Engine;
    use crate::error::EngineError;
    use crate::ids::Loader;
    use crate::paths::LauncherPaths;
    use crate::store::MemoryKeychain;
    use crate::types::ProgressSink;
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    struct NoopProgress;

    impl ProgressSink for NoopProgress {
        fn set(&self, _title: &str, _done: u64, _total: u64) {}
    }

    enum FakePackMode {
        Ok,
        BlockOnFile2,
        Escape,
    }

    struct FakePack {
        mode: FakePackMode,
        released: AtomicBool,
        gate: Notify,
    }

    impl FakePack {
        fn ok() -> Self {
            Self {
                mode: FakePackMode::Ok,
                released: AtomicBool::new(false),
                gate: Notify::new(),
            }
        }

        fn block_on_file_2() -> Self {
            Self {
                mode: FakePackMode::BlockOnFile2,
                released: AtomicBool::new(false),
                gate: Notify::new(),
            }
        }

        fn escape_override() -> Self {
            Self {
                mode: FakePackMode::Escape,
                released: AtomicBool::new(false),
                gate: Notify::new(),
            }
        }

        fn unblock(&self) {
            self.released.store(true, Ordering::Release);
            self.gate.notify_waiters();
        }

        fn catalog_file(&self, project_id: &CatalogProjectId, file_id: &str) -> CatalogFile {
            let (class, file_name, file_length) = if file_id == "pack" {
                (ContentClass::Modpacks, "pack.zip", 3)
            } else {
                (ContentClass::Mods, "jei.jar", 3)
            };
            CatalogFile {
                provider: ProviderId::CURSEFORGE,
                project_id: project_id.clone(),
                class,
                file_id: file_id.to_string(),
                display_name: file_name.to_string(),
                file_name: file_name.to_string(),
                release: CatalogRelease::Release,
                game_versions: vec!["1.20.1".into()],
                loaders: vec![Loader::Forge],
                file_length,
                download_count: 0,
                file_date: None,
            }
        }

        fn pack_manifest() -> PackManifestSpec {
            PackManifestSpec {
                name: "SF".into(),
                version: "1".into(),
                minecraft_version: "1.20.1".into(),
                loader: Loader::Forge,
                loader_version: Some("47.4.0".into()),
                files: vec![PackManifestFileSpec {
                    project_id: CatalogProjectId("1".into()),
                    file_id: "2".into(),
                    required: true,
                    class: ContentClass::Mods,
                }],
            }
        }
    }

    #[async_trait]
    impl CatalogProvider for FakePack {
        fn id(&self) -> ProviderId {
            ProviderId::CURSEFORGE
        }
        fn label(&self) -> &'static str {
            "FakePack"
        }
        fn supports(&self, _: ContentClass) -> bool {
            true
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
            _: &CatalogQuery,
        ) -> Result<CatalogPage<CatalogProject>, CatalogError> {
            Err(CatalogError::NotFound {
                kind: CatalogResource::Project,
                id: "-".into(),
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
            project_id: &CatalogProjectId,
            file_id: &str,
        ) -> Result<CatalogFile, CatalogError> {
            Ok(self.catalog_file(project_id, file_id))
        }
        async fn download(&self, file: &CatalogFile) -> Result<CatalogBlob, CatalogError> {
            if file.file_id == "2" && matches!(self.mode, FakePackMode::BlockOnFile2) {
                let notified = self.gate.notified();
                if !self.released.load(Ordering::Acquire) {
                    notified.await;
                }
            }
            if file.file_id == "pack" {
                Ok(CatalogBlob {
                    file_name: "pack.zip".into(),
                    bytes: b"zip".to_vec(),
                    sha1: None,
                })
            } else if file.file_id == "2" {
                Ok(CatalogBlob {
                    file_name: "jei.jar".into(),
                    bytes: b"jar".to_vec(),
                    sha1: None,
                })
            } else {
                Err(CatalogError::NotFound {
                    kind: CatalogResource::File,
                    id: file.file_id.clone(),
                })
            }
        }
        async fn parse_pack(&self, _: &[u8]) -> Result<PackManifestSpec, CatalogError> {
            Ok(Self::pack_manifest())
        }
        fn walk_overrides(
            &self,
            _: &[u8],
            visit: &mut dyn FnMut(PackOverride) -> Result<(), CatalogError>,
        ) -> Result<(), CatalogError> {
            let relative_path = match self.mode {
                FakePackMode::Escape => "../escape".into(),
                _ => "config/a.toml".into(),
            };
            visit(PackOverride {
                relative_path,
                bytes: b"ok".to_vec(),
            })
        }
        async fn resolve_required_deps(
            &self,
            _: &[CatalogFile],
            _: &str,
            _: Option<Loader>,
        ) -> Result<Vec<CatalogFile>, CatalogError> {
            Ok(vec![])
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
    async fn install_pack_writes_mods_and_overrides() {
        let (_root, engine) = test_engine();
        engine.add_provider(Arc::new(FakePack::ok()));
        let id = engine
            .install_pack(
                ProviderId::CURSEFORGE,
                &CatalogProjectId("p".into()),
                "pack",
                None,
                &NoopProgress,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let row = engine.get_instance(id).unwrap().unwrap();
        assert_eq!(row.loader, Loader::Forge);
        assert_eq!(row.loader_version.as_deref(), Some("47.4.0"));
        let mc = engine.paths.instance_minecraft(&row.slug);
        assert_eq!(std::fs::read(mc.join("mods/jei.jar")).unwrap(), b"jar");
        assert_eq!(std::fs::read(mc.join("config/a.toml")).unwrap(), b"ok");
    }

    #[tokio::test]
    async fn install_pack_cancel_after_create_deletes_instance() {
        let (_root, engine) = test_engine();
        let engine = Arc::new(engine);
        let fake = Arc::new(FakePack::block_on_file_2());
        engine.add_provider(fake.clone());
        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        let join = tokio::spawn({
            let engine = Arc::clone(&engine);
            let cancel = cancel.clone();
            async move {
                engine
                    .install_pack(
                        ProviderId::CURSEFORGE,
                        &CatalogProjectId("p".into()),
                        "pack",
                        None,
                        &NoopProgress,
                        &cancel,
                    )
                    .await
            }
        });
        while engine.list_instances().unwrap().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        cancel2.cancel();
        fake.unblock();
        let err = join.await.unwrap().unwrap_err();
        assert!(matches!(err, EngineError::Cancelled));
        assert!(engine.list_instances().unwrap().is_empty());
    }

    #[tokio::test]
    async fn install_pack_rejects_override_escape() {
        let (_root, engine) = test_engine();
        engine.add_provider(Arc::new(FakePack::escape_override()));
        let err = engine
            .install_pack(
                ProviderId::CURSEFORGE,
                &CatalogProjectId("p".into()),
                "pack",
                None,
                &NoopProgress,
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(engine.list_instances().unwrap().is_empty());
        let _ = err;
    }

    #[tokio::test]
    async fn install_pack_while_locked_is_busy() {
        let (_root, engine) = test_engine();
        *engine.installing.lock() = true;
        let err = engine
            .install_pack(
                ProviderId::CURSEFORGE,
                &CatalogProjectId("p".into()),
                "pack",
                None,
                &NoopProgress,
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, EngineError::InstanceBusy));
    }
}
