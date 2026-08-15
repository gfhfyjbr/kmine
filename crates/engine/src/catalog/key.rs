use super::provider::ProviderId;
use super::types::{CatalogCredentials, CatalogError};
use crate::Engine;
use crate::error::EngineError;
use crate::http::HttpFiles;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

const SECRET_ID: &str = "catalog/curseforge";
const REFRESH_INTERVAL: Duration = Duration::from_secs(3600);

#[derive(Debug, Deserialize)]
struct CfApiKeyResponse {
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(default)]
    #[allow(dead_code)]
    source: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct CatalogKeySecret {
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(rename = "updatedAt")]
    updated_at: i64,
}

impl Engine {
    pub fn set_catalog_backend_url(&self, url: impl Into<String>) {
        let mut base = url.into();
        while base.ends_with('/') {
            base.pop();
        }
        *self.catalog_backend_url.lock() = base;
    }

    pub async fn refresh_catalog_key_once(&self) -> Result<(), EngineError> {
        let provider = match self.provider(ProviderId::CURSEFORGE) {
            Ok(p) => p,
            Err(CatalogError::UnknownProvider) => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        refresh_catalog_key(
            &self.store,
            &self.master_key,
            &self.catalog_backend_url,
            self.catalog_backend_token.as_deref(),
            &provider,
        )
        .await
    }

    pub fn start_catalog_key_refresh(&self) {
        let Ok(provider) = self.provider(ProviderId::CURSEFORGE) else {
            return;
        };

        if let Some(api_key) = self.read_catalog_secret_api_key() {
            provider.set_credentials(CatalogCredentials::ApiKey(api_key));
        }

        let store = Arc::clone(&self.store);
        let master_key = self.master_key;
        let url = Arc::clone(&self.catalog_backend_url);
        let token = self.catalog_backend_token.clone();
        let providers = Arc::clone(&self.providers);

        self.rt.spawn(async move {
            let mut ticker = tokio::time::interval(REFRESH_INTERVAL);
            loop {
                ticker.tick().await;
                let provider = {
                    let guard = providers.lock();
                    guard
                        .iter()
                        .find(|p| p.id() == ProviderId::CURSEFORGE)
                        .cloned()
                };
                let Some(provider) = provider else {
                    continue;
                };
                let _ = refresh_catalog_key(&store, &master_key, &url, token.as_deref(), &provider)
                    .await;
            }
        });
    }

    fn read_catalog_secret_api_key(&self) -> Option<String> {
        let raw = self
            .store
            .lock()
            .get_secret(&self.master_key, SECRET_ID)
            .ok()??;
        parse_secret_api_key(&raw)
    }
}

async fn refresh_catalog_key(
    store: &parking_lot::Mutex<crate::store::Store>,
    master_key: &[u8; 32],
    catalog_backend_url: &parking_lot::Mutex<String>,
    token: Option<&str>,
    provider: &Arc<dyn super::CatalogProvider>,
) -> Result<(), EngineError> {
    let base = catalog_backend_url.lock().clone();
    let url = format!("{base}/get_cf_api_key");

    let http = HttpFiles::new()?;
    let mut req = http.client.get(&url);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }

    let response = req.send().await.map_err(|err| {
        EngineError::io(
            std::path::PathBuf::from(&url),
            std::io::Error::other(err.to_string()),
        )
    })?;

    let status = response.status();
    if !status.is_success() {
        return Err(CatalogError::Http {
            url,
            status: status.as_u16(),
        }
        .into());
    }

    let body = response.bytes().await.map_err(|err| {
        EngineError::io(
            std::path::PathBuf::from(&url),
            std::io::Error::other(err.to_string()),
        )
    })?;

    // Parse without embedding response body in errors (avoids leaking apiKey).
    let parsed: CfApiKeyResponse = serde_json::from_slice(&body).map_err(|err| {
        EngineError::io(
            std::path::PathBuf::from(&url),
            std::io::Error::other(err.to_string()),
        )
    })?;

    if parsed.api_key.is_empty() {
        return Err(CatalogError::Message("empty api key".into()).into());
    }

    let secret = CatalogKeySecret {
        api_key: parsed.api_key.clone(),
        updated_at: crate::now_ms(),
    };
    let plaintext = serde_json::to_vec(&secret).map_err(|err| {
        EngineError::io(
            std::path::PathBuf::from(SECRET_ID),
            std::io::Error::other(err.to_string()),
        )
    })?;

    store.lock().put_secret(master_key, SECRET_ID, &plaintext)?;
    provider.set_credentials(CatalogCredentials::ApiKey(parsed.api_key));
    Ok(())
}

fn parse_secret_api_key(raw: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let key = v.get("apiKey")?.as_str()?;
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

pub(crate) fn default_catalog_backend_url() -> String {
    let mut base = std::env::var("KMINE_BACKEND_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:8787".into());
    while base.ends_with('/') {
        base.pop();
    }
    base
}

pub(crate) fn catalog_backend_token_from_env() -> Option<String> {
    std::env::var("KMINE_BACKEND_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::provider::CatalogProvider;
    use crate::catalog::types::{
        CatalogBlob, CatalogCategory, CatalogError, CatalogFile, CatalogFileFilter, CatalogPage,
        CatalogProject, CatalogProjectDetail, CatalogProjectId, CatalogQuery, CatalogResource,
        ContentClass, PackManifestSpec, PackOverride,
    };
    use crate::ids::Loader;
    use crate::paths::LauncherPaths;
    use crate::store::MemoryKeychain;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const FIXTURE_KEY: &str = "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm";

    #[derive(Default)]
    struct CredFake {
        has_creds: Mutex<bool>,
    }

    #[async_trait]
    impl CatalogProvider for CredFake {
        fn id(&self) -> ProviderId {
            ProviderId::CURSEFORGE
        }
        fn label(&self) -> &'static str {
            "CredFake"
        }
        fn supports(&self, class: ContentClass) -> bool {
            class == ContentClass::Mods
        }
        fn set_credentials(&self, _: CatalogCredentials) {
            *self.has_creds.lock().unwrap() = true;
        }
        fn has_credentials(&self) -> bool {
            *self.has_creds.lock().unwrap()
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
        async fn file(&self, _: &CatalogProjectId, _: &str) -> Result<CatalogFile, CatalogError> {
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
    async fn refresh_writes_secret_and_sets_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/get_cf_api_key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "apiKey": FIXTURE_KEY,
                "source": "test"
            })))
            .mount(&server)
            .await;
        let (_root, engine) = test_engine();
        let fake = Arc::new(CredFake::default());
        engine.add_provider(fake.clone());
        engine.set_catalog_backend_url(server.uri());
        engine.refresh_catalog_key_once().await.unwrap();
        assert!(fake.has_credentials());
        let raw = engine
            .store
            .lock()
            .get_secret(&engine.master_key, "catalog/curseforge")
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["apiKey"], FIXTURE_KEY);
        assert!(v["updatedAt"].as_i64().unwrap() > 0);
    }

    #[tokio::test]
    async fn refresh_503_keeps_old_secret() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/get_cf_api_key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "apiKey": FIXTURE_KEY,
                "source": "test"
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/get_cf_api_key"))
            .respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({
                "error": "key unavailable"
            })))
            .mount(&server)
            .await;

        let (_root, engine) = test_engine();
        let fake = Arc::new(CredFake::default());
        engine.add_provider(fake.clone());
        engine.set_catalog_backend_url(server.uri());

        engine.refresh_catalog_key_once().await.unwrap();
        let raw_before = engine
            .store
            .lock()
            .get_secret(&engine.master_key, "catalog/curseforge")
            .unwrap()
            .unwrap();

        let err = engine.refresh_catalog_key_once().await.unwrap_err();
        assert!(matches!(
            err,
            EngineError::Catalog(CatalogError::Http { status: 503, .. })
        ));

        let raw_after = engine
            .store
            .lock()
            .get_secret(&engine.master_key, "catalog/curseforge")
            .unwrap()
            .unwrap();
        assert_eq!(raw_before, raw_after);
        let v: serde_json::Value = serde_json::from_slice(&raw_after).unwrap();
        assert_eq!(v["apiKey"], FIXTURE_KEY);
    }

    #[tokio::test]
    async fn refresh_error_display_hides_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/get_cf_api_key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "apiKey": FIXTURE_KEY,
                "source": "test"
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/get_cf_api_key"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(format!(r#"not-json {FIXTURE_KEY}"#)),
            )
            .mount(&server)
            .await;

        let (_root, engine) = test_engine();
        let fake = Arc::new(CredFake::default());
        engine.add_provider(fake);
        engine.set_catalog_backend_url(server.uri());

        engine.refresh_catalog_key_once().await.unwrap();
        let err = engine.refresh_catalog_key_once().await.unwrap_err();
        let display = err.to_string();
        assert!(
            !display.contains("$2a$10$"),
            "error display leaked key material: {display}"
        );
        let debug = format!("{err:?}");
        assert!(
            !debug.contains(FIXTURE_KEY),
            "error debug leaked key material"
        );

        let http_err = EngineError::Catalog(CatalogError::Http {
            url: format!("{}/get_cf_api_key", server.uri()),
            status: 503,
        });
        assert!(!http_err.to_string().contains("$2a$10$"));
        assert!(!http_err.to_string().contains(FIXTURE_KEY));
    }

    #[tokio::test]
    async fn refresh_without_provider_is_ok() {
        let (_root, engine) = test_engine();
        engine.refresh_catalog_key_once().await.unwrap();
    }
}
