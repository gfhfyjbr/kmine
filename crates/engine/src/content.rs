use crate::Engine;
use crate::error::EngineError;
use crate::ids::InstanceId;
use crate::instance_not_found;
use crate::paths::LauncherPaths;
use crate::types::{ContentEntry, ContentFolder};
use std::io;
use std::path::{Path, PathBuf};

const DISABLED_SUFFIX: &str = ".disabled";

const CONTENT_FOLDERS: [ContentFolder; 3] = [
    ContentFolder::Mods,
    ContentFolder::Resourcepacks,
    ContentFolder::Shaderpacks,
];

pub fn set_content_enabled_on_disk(path: &Path, enabled: bool) -> Result<(), EngineError> {
    let name = file_name(path)?;
    if enabled {
        if !name.ends_with(DISABLED_SUFFIX) {
            return Ok(());
        }
        let stripped = name
            .strip_suffix(DISABLED_SUFFIX)
            .expect("suffix already checked");
        rename_to(path, stripped)
    } else {
        if name.ends_with(DISABLED_SUFFIX) {
            return Ok(());
        }
        rename_to(path, &format!("{name}{DISABLED_SUFFIX}"))
    }
}

impl Engine {
    pub fn list_content(
        &self,
        id: InstanceId,
        folder: ContentFolder,
    ) -> Result<Vec<ContentEntry>, EngineError> {
        let slug = self.instance_slug(id)?;
        let dir = self.paths.instance_minecraft(&slug).join(folder.dir_name());
        match std::fs::create_dir_all(&dir) {
            Ok(()) => {}
            Err(e) => return Err(EngineError::io(&dir, e)),
        }
        list_content_dir(&dir)
    }

    pub fn set_content_enabled(
        &self,
        id: InstanceId,
        path: &Path,
        enabled: bool,
    ) -> Result<(), EngineError> {
        let slug = self.instance_slug(id)?;
        ensure_under_content_folder(&self.paths, &slug, path)?;
        set_content_enabled_on_disk(path, enabled)
    }

    pub fn delete_content(&self, id: InstanceId, path: &Path) -> Result<(), EngineError> {
        let slug = self.instance_slug(id)?;
        let folder = content_folder_for_path(&self.paths, &slug, path).ok_or_else(|| {
            EngineError::io(
                path,
                io::Error::new(io::ErrorKind::InvalidInput, "path escapes content folder"),
            )
        })?;
        if !path.starts_with(&folder) {
            return Err(EngineError::io(
                path,
                io::Error::new(io::ErrorKind::InvalidInput, "path escapes content folder"),
            ));
        }
        std::fs::remove_file(path).map_err(|e| EngineError::io(path, e))
    }

    fn instance_slug(&self, id: InstanceId) -> Result<String, EngineError> {
        let store = self.store.lock();
        match store.get_instance(id)? {
            Some(row) => Ok(row.slug),
            None => Err(instance_not_found(&self.paths)),
        }
    }
}

fn list_content_dir(dir: &Path) -> Result<Vec<ContentEntry>, EngineError> {
    let read = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(EngineError::io(dir, e)),
    };
    let mut entries = Vec::new();
    for ent in read {
        let ent = ent.map_err(|e| EngineError::io(dir, e))?;
        let os_name = ent.file_name();
        let Some(name) = os_name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let file_type = ent
            .file_type()
            .map_err(|e| EngineError::io(ent.path(), e))?;
        if file_type.is_dir() {
            continue;
        }
        let enabled = !name.ends_with(DISABLED_SUFFIX);
        let display = name
            .strip_suffix(DISABLED_SUFFIX)
            .unwrap_or(name)
            .to_string();
        entries.push(ContentEntry {
            path: ent.path(),
            name: display,
            enabled,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn content_folder_for_path(paths: &LauncherPaths, slug: &str, path: &Path) -> Option<PathBuf> {
    let mc = paths.instance_minecraft(slug);
    CONTENT_FOLDERS
        .into_iter()
        .map(|folder| mc.join(folder.dir_name()))
        .find(|dir| path.starts_with(dir))
}

fn ensure_under_content_folder(
    paths: &LauncherPaths,
    slug: &str,
    path: &Path,
) -> Result<PathBuf, EngineError> {
    content_folder_for_path(paths, slug, path).ok_or_else(|| {
        EngineError::io(
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "path escapes content folder"),
        )
    })
}

fn file_name(path: &Path) -> Result<&str, EngineError> {
    path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        EngineError::io(
            path,
            io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"),
        )
    })
}

fn rename_to(path: &Path, new_name: &str) -> Result<(), EngineError> {
    let target = path.with_file_name(new_name);
    if target.exists() {
        return Err(EngineError::io(
            &target,
            io::Error::new(io::ErrorKind::AlreadyExists, "already exists"),
        ));
    }
    std::fs::rename(path, &target).map_err(|e| EngineError::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::set_content_enabled_on_disk;
    use crate::ids::Loader;
    use crate::store::MemoryKeychain;
    use crate::types::{ContentFolder, CreateInstance};
    use crate::{Engine, EngineError, LauncherPaths};
    use std::io;
    use std::path::Path;

    #[test]
    fn enable_disable_rename() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        crate::instance::create_instance_dirs(&paths, "One").unwrap();
        let mods = paths.instance_minecraft("One").join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        let jar = mods.join("sodium.jar");
        std::fs::write(&jar, b"jar").unwrap();

        set_content_enabled_on_disk(&jar, false).unwrap();
        assert!(mods.join("sodium.jar.disabled").is_file());
        set_content_enabled_on_disk(&mods.join("sodium.jar.disabled"), true).unwrap();
        assert!(mods.join("sodium.jar").is_file());
    }

    #[tokio::test]
    async fn list_skips_dotfiles_and_dirs() {
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
        let mods = engine.paths.instance_minecraft("One").join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("sodium.jar"), b"").unwrap();
        std::fs::write(mods.join("iris.jar.disabled"), b"").unwrap();
        std::fs::write(mods.join(".DS_Store"), b"").unwrap();
        std::fs::create_dir_all(mods.join("subdir")).unwrap();
        std::fs::write(mods.join("subdir").join("nested.jar"), b"").unwrap();

        let listed = engine.list_content(id, ContentFolder::Mods).unwrap();
        let names: Vec<_> = listed
            .iter()
            .map(|e| (e.name.as_str(), e.enabled))
            .collect();
        assert_eq!(names, vec![("iris.jar", false), ("sodium.jar", true)]);
    }

    #[tokio::test]
    async fn delete_rejects_escape() {
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
        let err = engine
            .delete_content(id, Path::new("/etc/passwd"))
            .unwrap_err();
        assert!(matches!(err, EngineError::Io { .. }));
    }

    #[test]
    fn disable_errors_if_target_exists() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let enabled = dir.join("sodium.jar");
        let disabled = dir.join("sodium.jar.disabled");
        std::fs::write(&enabled, b"a").unwrap();
        std::fs::write(&disabled, b"b").unwrap();
        let err = set_content_enabled_on_disk(&enabled, false).unwrap_err();
        match err {
            EngineError::Io { source, .. } => {
                assert_eq!(source.kind(), io::ErrorKind::AlreadyExists);
            }
            other => panic!("expected Io AlreadyExists, got {other:?}"),
        }
        assert!(enabled.is_file());
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
