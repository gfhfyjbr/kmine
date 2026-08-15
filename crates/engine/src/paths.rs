use crate::error::EngineError;
use std::io;
use std::path::{Path, PathBuf};

pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf, EngineError> {
    if Path::new(rel).is_absolute() {
        return Err(unsafe_path(base, rel));
    }
    let mut dest = base.to_path_buf();
    let mut pushed = false;
    for part in rel.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || Path::new(part).is_absolute() || Path::new(part).has_root() {
            return Err(unsafe_path(base, rel));
        }
        dest.push(part);
        pushed = true;
    }
    if !pushed {
        return Err(unsafe_path(base, rel));
    }
    Ok(dest)
}

fn unsafe_path(base: &Path, rel: &str) -> EngineError {
    EngineError::io(
        base.join(rel),
        io::Error::new(io::ErrorKind::InvalidInput, "unsafe path"),
    )
}

#[derive(Debug, Clone)]
pub struct LauncherPaths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub instances: PathBuf,
    pub cache_meta: PathBuf,
    pub cache_libraries: PathBuf,
    pub cache_assets_indexes: PathBuf,
    pub cache_assets_objects: PathBuf,
    pub cache_assets_virtual: PathBuf,
    pub cache_runtime: PathBuf,
    pub cache_natives: PathBuf,
    pub cache_skins: PathBuf,
    pub cache_catalog_files: PathBuf,
    pub cache_catalog_images: PathBuf,
}

impl LauncherPaths {
    pub fn new(root: PathBuf) -> Self {
        let cache = root.join("cache");
        let assets = cache.join("assets");
        Self {
            db: root.join("kmine.db"),
            instances: root.join("instances"),
            cache_meta: cache.join("meta"),
            cache_libraries: cache.join("libraries"),
            cache_assets_indexes: assets.join("indexes"),
            cache_assets_objects: assets.join("objects"),
            cache_assets_virtual: assets.join("virtual").join("legacy"),
            cache_runtime: cache.join("runtime"),
            cache_natives: cache.join("natives"),
            cache_skins: cache.join("skins"),
            cache_catalog_files: cache.join("catalog").join("files"),
            cache_catalog_images: cache.join("catalog").join("images"),
            root,
        }
    }

    pub fn default_root() -> PathBuf {
        directories::BaseDirs::new()
            .map(|b| b.data_dir().join("kmine"))
            .unwrap_or_else(|| PathBuf::from("kmine-data"))
    }

    pub fn create_dirs(&self) -> Result<(), EngineError> {
        for dir in [
            &self.root,
            &self.instances,
            &self.cache_meta,
            &self.cache_libraries,
            &self.cache_assets_indexes,
            &self.cache_assets_objects,
            &self.cache_assets_virtual,
            &self.cache_runtime,
            &self.cache_natives,
            &self.cache_skins,
            &self.cache_catalog_files,
            &self.cache_catalog_images,
        ] {
            std::fs::create_dir_all(dir).map_err(|e| EngineError::io(dir, e))?;
        }
        Ok(())
    }

    pub fn instance_dir(&self, slug: &str) -> PathBuf {
        self.instances.join(slug)
    }

    pub fn instance_minecraft(&self, slug: &str) -> PathBuf {
        self.instance_dir(slug).join(".minecraft")
    }
}

#[cfg(test)]
mod tests {
    use super::LauncherPaths;
    use std::path::PathBuf;

    #[test]
    fn paths_use_kmine_db_name() {
        let paths = LauncherPaths::new(PathBuf::from("/tmp/kmine-test"));
        assert_eq!(paths.db.file_name().unwrap(), "kmine.db");
        assert!(paths.instances.ends_with("instances"));
        assert!(
            paths.cache_runtime.ends_with("cache/runtime")
                || paths.cache_runtime.ends_with("cache\\runtime")
        );
    }

    #[test]
    fn create_dirs_makes_instance_and_cache_trees() {
        let dir = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(dir.path().to_path_buf());
        paths.create_dirs().unwrap();
        assert!(paths.instances.is_dir());
        assert!(paths.cache_meta.is_dir());
        assert!(paths.cache_libraries.is_dir());
        assert!(paths.cache_assets_indexes.is_dir());
        assert!(paths.cache_assets_objects.is_dir());
        assert!(paths.cache_assets_virtual.is_dir());
        assert!(paths.cache_runtime.is_dir());
        assert!(paths.cache_natives.is_dir());
        assert!(paths.cache_skins.is_dir());
        assert!(paths.cache_catalog_files.is_dir());
        assert!(paths.cache_catalog_images.is_dir());
    }

    #[test]
    fn safe_join_rejects_escape() {
        use std::path::Path;
        let base = Path::new("/data/kmine/cache/libraries");
        assert!(super::safe_join(base, "com/mojang/a.jar").is_ok());
        assert!(super::safe_join(base, "../escape.jar").is_err());
        assert!(super::safe_join(base, "/tmp/evil.jar").is_err());
        assert!(super::safe_join(base, "com/../../etc/passwd").is_err());
        assert!(super::safe_join(base, "").is_err());
        assert!(super::safe_join(base, ".").is_err());
    }

    #[test]
    fn instance_minecraft_is_under_slug() {
        let paths = LauncherPaths::new(PathBuf::from("/tmp/kmine-test"));
        let mc = paths.instance_minecraft("My Pack");
        assert!(
            mc.ends_with("instances/My Pack/.minecraft")
                || mc.ends_with("instances\\My Pack\\.minecraft")
        );
    }
}
