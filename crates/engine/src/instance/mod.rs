use crate::error::EngineError;
use crate::paths::LauncherPaths;

pub fn slug_from_name(name: &str) -> String {
    let stripped: String = name
        .trim()
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    if stripped.is_empty() {
        "instance".into()
    } else {
        stripped
    }
}

pub fn unique_slug(desired: &str, taken: &[String]) -> String {
    if !taken.iter().any(|s| s == desired) {
        return desired.to_string();
    }
    for n in 2..=999 {
        let candidate = format!("{desired} ({n})");
        if !taken.iter().any(|s| s == &candidate) {
            return candidate;
        }
    }
    format!("{desired} ({})", uuid::Uuid::new_v4().as_hyphenated())
}

pub fn create_instance_dirs(paths: &LauncherPaths, slug: &str) -> Result<(), EngineError> {
    let dir = paths.instance_minecraft(slug);
    std::fs::create_dir_all(&dir).map_err(|e| EngineError::io(&dir, e))
}

pub fn rename_instance_dir(paths: &LauncherPaths, old: &str, new: &str) -> Result<(), EngineError> {
    let from = paths.instance_dir(old);
    let to = paths.instance_dir(new);
    std::fs::rename(&from, &to).map_err(|e| EngineError::io(&from, e))
}

pub fn delete_instance_dir(paths: &LauncherPaths, slug: &str) -> Result<(), EngineError> {
    let dir = paths.instance_dir(slug);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(EngineError::io(dir, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::{create_instance_dirs, rename_instance_dir, slug_from_name, unique_slug};
    use crate::paths::LauncherPaths;

    #[test]
    fn slug_strips_forbidden_chars() {
        assert_eq!(slug_from_name("  My/Pack?  "), "MyPack");
        assert_eq!(slug_from_name("   "), "instance");
    }

    #[test]
    fn unique_slug_adds_numeric_suffix() {
        let taken = vec!["Foo".into(), "Foo (2)".into()];
        assert_eq!(unique_slug("Foo", &taken), "Foo (3)");
        assert_eq!(unique_slug("Bar", &taken), "Bar");
    }

    #[test]
    fn create_and_rename_instance_dirs() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        create_instance_dirs(&paths, "Alpha").unwrap();
        assert!(paths.instance_minecraft("Alpha").is_dir());
        rename_instance_dir(&paths, "Alpha", "Beta").unwrap();
        assert!(!paths.instance_dir("Alpha").exists());
        assert!(paths.instance_minecraft("Beta").is_dir());
    }
}
