//! Catalog types, provider trait surface, and manifest helpers.

pub mod cache;
pub mod install;
pub mod key;
pub mod loader_id;
pub mod provider;
pub mod query;
pub mod types;

pub use loader_id::parse_manifest_loader;
pub use provider::{CatalogProvider, ProviderId};
pub use types::{
    CatalogBlob, CatalogCategory, CatalogCredentials, CatalogError, CatalogFile, CatalogFileFilter,
    CatalogPage, CatalogProject, CatalogProjectDetail, CatalogProjectId, CatalogQuery,
    CatalogRelease, CatalogResource, CatalogSort, ContentClass, PackManifestFileSpec,
    PackManifestSpec, PackOverride,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Loader;

    #[test]
    fn parse_manifest_loader_table() {
        let cases = [
            ("forge-47.4.0", "1.20.1", Loader::Forge, "47.4.0"),
            ("fabric-0.16.9", "1.21.1", Loader::Fabric, "0.16.9"),
            ("fabric-0.16.9-1.21.1", "1.21.1", Loader::Fabric, "0.16.9"),
            (
                "fabric-0.16.9-1.21.1",
                "1.20.1",
                Loader::Fabric,
                "0.16.9-1.21.1",
            ),
            ("neoforge-21.1.66", "1.21.1", Loader::NeoForge, "21.1.66"),
            ("neoforge-47.1.106", "1.20.1", Loader::NeoForge, "47.1.106"),
            ("quilt-0.27.1", "1.21.1", Loader::Quilt, "0.27.1"),
        ];
        for (id, mc, loader, ver) in cases {
            assert_eq!(
                parse_manifest_loader(id, mc).unwrap(),
                (loader, ver.to_string()),
                "{id}"
            );
        }
        assert!(matches!(
            parse_manifest_loader("liteloader-1.12", "1.12"),
            Err(CatalogError::UnsupportedLoader { .. })
        ));
    }

    #[test]
    fn catalog_error_display_has_no_key_looking_secret() {
        let err = CatalogError::Http {
            url: "http://127.0.0.1:8787/get_cf_api_key".into(),
            status: 503,
        };
        let s = err.to_string();
        assert!(!s.contains("apiKey"));
        assert!(!s.contains("$2a$10$"));
    }
}
