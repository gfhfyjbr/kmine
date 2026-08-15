use crate::Error;
use crate::search::{CategoryFilter, FileFilter, SearchQuery};
use crate::types::{
    Category, DEFAULT_BASE_URL, File, MINECRAFT_GAME_ID, Mod, Page, Pagination, SortOrder,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const RETRIES: u32 = 2;
const RETRY_DELAY: Duration = Duration::from_millis(1000);

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    request_timeout: Duration,
    #[allow(dead_code)] // reserved for download API
    download_timeout: Duration,
}

pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: String,
    user_agent: String,
    connect_timeout: Duration,
    request_timeout: Duration,
    download_timeout: Duration,
}

impl Client {
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        Self::builder().api_key(api_key).build()
    }

    pub fn builder() -> ClientBuilder {
        ClientBuilder {
            api_key: None,
            base_url: DEFAULT_BASE_URL.to_string(),
            user_agent: format!("kmine-curseforge/{}", env!("CARGO_PKG_VERSION")),
            connect_timeout: CONNECT_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            download_timeout: DOWNLOAD_TIMEOUT,
        }
    }

    pub async fn categories(&self, filter: CategoryFilter) -> Result<Vec<Category>, Error> {
        let mut q = vec![("gameId".into(), MINECRAFT_GAME_ID.to_string())];
        match filter {
            CategoryFilter::All => {}
            CategoryFilter::ClassesOnly => q.push(("classesOnly".into(), "true".into())),
            CategoryFilter::ChildrenOf(class) => q.push(("classId".into(), class.0.to_string())),
        }
        self.get_data("/v1/categories", &q).await
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<Page<Mod>, Error> {
        query.validate()?;
        let mut q = vec![
            ("gameId".into(), MINECRAFT_GAME_ID.to_string()),
            ("classId".into(), query.class.0.to_string()),
            ("sortField".into(), query.sort_field.as_u8().to_string()),
            (
                "sortOrder".into(),
                match query.sort_order {
                    SortOrder::Asc => "asc".into(),
                    SortOrder::Desc => "desc".into(),
                },
            ),
            ("index".into(), query.index.to_string()),
            ("pageSize".into(), query.page_size.to_string()),
        ];
        if let Some(text) = query.search.as_ref().filter(|s| !s.is_empty()) {
            q.push(("searchFilter".into(), text.clone()));
        }
        if !query.categories.is_empty() {
            q.push((
                "categoryIds".into(),
                serde_json::to_string(&query.categories).unwrap(),
            ));
        }
        if !query.game_versions.is_empty() {
            q.push((
                "gameVersions".into(),
                serde_json::to_string(&query.game_versions).unwrap(),
            ));
        }
        if !query.loaders.is_empty() {
            let ids: Vec<u8> = query.loaders.iter().map(|l| l.as_u8()).collect();
            q.push((
                "modLoaderTypes".into(),
                serde_json::to_string(&ids).unwrap(),
            ));
        }
        if let Some(slug) = query.slug.as_ref().filter(|s| !s.is_empty()) {
            q.push(("slug".into(), slug.clone()));
        }
        if let Some(id) = query.author_id {
            q.push(("primaryAuthorId".into(), id.to_string()));
        }
        if let Some(id) = query.game_version_type_id {
            q.push(("gameVersionTypeId".into(), id.to_string()));
        }
        self.get_page("/v2/mods/search", &q).await
    }

    pub async fn get_mod(&self, mod_id: u32) -> Result<Mod, Error> {
        self.get_data(&format!("/v2/mods/{mod_id}"), &[])
            .await
            .map_err(|e| map_http(e, Some((crate::ResourceKind::Mod, mod_id))))
    }

    pub async fn get_mods(&self, mod_ids: &[u32]) -> Result<Vec<Mod>, Error> {
        if mod_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for chunk in mod_ids.chunks(crate::BATCH_SIZE) {
            #[derive(serde::Serialize)]
            struct Body<'a> {
                #[serde(rename = "modIds")]
                mod_ids: &'a [u32],
            }
            let part: Vec<Mod> = self
                .post_data("/v2/mods/get-mods-by-ids", &Body { mod_ids: chunk })
                .await?;
            out.extend(part);
        }
        Ok(out)
    }

    pub async fn description(&self, mod_id: u32) -> Result<String, Error> {
        self.get_data(&format!("/v1/mods/{mod_id}/description"), &[])
            .await
            .map_err(|e| map_http(e, Some((crate::ResourceKind::Mod, mod_id))))
    }

    pub async fn files(&self, mod_id: u32, filter: &FileFilter) -> Result<Page<File>, Error> {
        if !(1..=crate::MAX_PAGE_SIZE).contains(&filter.page_size) {
            return Err(Error::InvalidQuery {
                message: "pageSize must be 1..=50",
            });
        }
        if filter.index.saturating_add(filter.page_size) > crate::MAX_INDEX_PLUS_PAGE {
            return Err(Error::InvalidQuery {
                message: "index + pageSize exceeds 10000",
            });
        }
        let mut q = vec![
            ("index".into(), filter.index.to_string()),
            ("pageSize".into(), filter.page_size.to_string()),
        ];
        if let Some(v) = &filter.game_version {
            q.push(("gameVersion".into(), v.clone()));
        }
        if let Some(id) = filter.game_version_type_id {
            q.push(("gameVersionTypeId".into(), id.to_string()));
        }
        if let Some(loader) = filter.loader {
            q.push(("modLoaderType".into(), loader.as_u8().to_string()));
        }
        if let Some(cc) = filter.client_compatible {
            q.push((
                "clientCompatible".into(),
                if cc { "true" } else { "false" }.into(),
            ));
        }
        self.get_page(&format!("/v1/mods/{mod_id}/files"), &q).await
    }

    pub async fn get_file(&self, mod_id: u32, file_id: u32) -> Result<File, Error> {
        self.get_data(&format!("/v1/mods/{mod_id}/files/{file_id}"), &[])
            .await
            .map_err(|e| map_http(e, Some((crate::ResourceKind::File, file_id))))
    }

    pub async fn get_files(&self, file_ids: &[u32]) -> Result<Vec<File>, Error> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for chunk in file_ids.chunks(crate::BATCH_SIZE) {
            #[derive(serde::Serialize)]
            struct Body<'a> {
                #[serde(rename = "fileIds")]
                file_ids: &'a [u32],
            }
            let part: Vec<File> = self
                .post_data("/v1/mods/files", &Body { file_ids: chunk })
                .await?;
            out.extend(part);
        }
        Ok(out)
    }

    pub async fn changelog(&self, mod_id: u32, file_id: u32) -> Result<String, Error> {
        self.get_data(
            &format!("/v1/mods/{mod_id}/files/{file_id}/changelog"),
            &[],
        )
        .await
        .map_err(|e| map_http(e, Some((crate::ResourceKind::File, file_id))))
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl ClientBuilder {
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }
    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = d;
        self
    }
    pub fn request_timeout(mut self, d: Duration) -> Self {
        self.request_timeout = d;
        self
    }
    pub fn download_timeout(mut self, d: Duration) -> Self {
        self.download_timeout = d;
        self
    }
    pub fn build(self) -> Result<Client, Error> {
        let api_key = self.api_key.unwrap_or_default();
        if api_key.is_empty() {
            return Err(Error::InvalidQuery {
                message: "empty api key",
            });
        }
        let http = reqwest::Client::builder()
            .user_agent(self.user_agent)
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .build()
            .map_err(|_| Error::Builder {
                message: "failed to build reqwest client",
            })?;
        Ok(Client {
            http,
            api_key,
            base_url: self.base_url.trim_end_matches('/').to_string(),
            request_timeout: self.request_timeout,
            download_timeout: self.download_timeout,
        })
    }
}

#[derive(serde::Deserialize)]
struct Envelope<T> {
    data: T,
    #[serde(default)]
    #[allow(dead_code)] // used by get_page
    pagination: Option<Pagination>,
}

impl Client {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    pub(crate) async fn get_data<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<T, Error> {
        let env: Envelope<T> = self.send_retry("GET", path, query, None::<&()>).await?;
        Ok(env.data)
    }

    pub(crate) async fn get_page<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<Page<T>, Error> {
        let env: Envelope<Vec<T>> = self.send_retry("GET", path, query, None::<&()>).await?;
        Ok(Page {
            data: env.data,
            pagination: env.pagination.unwrap_or(Pagination {
                index: 0,
                page_size: 0,
                result_count: 0,
                total_count: 0,
            }),
        })
    }

    pub(crate) async fn post_data<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        let env: Envelope<T> = self.send_retry("POST", path, &[], Some(body)).await?;
        Ok(env.data)
    }

    pub(crate) async fn send_retry<T: DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        path: &str,
        query: &[(String, String)],
        body: Option<&B>,
    ) -> Result<T, Error> {
        let url = self.url(path);
        let mut last = Error::Http {
            url: url.clone(),
            status: 0,
        };
        for attempt in 0..=RETRIES {
            match self.send_once::<T, B>(method, &url, query, body).await {
                Ok(v) => return Ok(v),
                Err(err) if attempt < RETRIES && retryable(&err) => {
                    if !matches!(err, Error::Http { status: 429, .. }) {
                        tokio::time::sleep(RETRY_DELAY).await;
                    }
                    last = err;
                }
                Err(err) => return Err(err),
            }
        }
        Err(last)
    }

    async fn send_once<T: DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        url: &str,
        query: &[(String, String)],
        body: Option<&B>,
    ) -> Result<T, Error> {
        let mut req = match method {
            "POST" => self.http.post(url),
            _ => self.http.get(url),
        };
        req = req
            .header("x-api-key", &self.api_key)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(self.request_timeout)
            .query(query);
        if let Some(body) = body {
            req = req.json(body);
        }
        let response = req.send().await.map_err(|err| Error::Decode {
            url: url.to_string(),
            message: err.to_string(),
        })?;
        let status = response.status();
        if status.as_u16() == 429 {
            let delay = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(RETRY_DELAY);
            tokio::time::sleep(delay).await;
            return Err(Error::Http {
                url: url.to_string(),
                status: 429,
            });
        }
        if !status.is_success() {
            return Err(Error::Http {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        response.json::<T>().await.map_err(|err| Error::Decode {
            url: url.to_string(),
            message: err.to_string(),
        })
    }
}

fn map_http(err: Error, not_found: Option<(crate::ResourceKind, u32)>) -> Error {
    match (&err, not_found) {
        (Error::Http { status: 404, .. }, Some((kind, id))) => Error::NotFound { kind, id },
        _ => err,
    }
}

fn retryable(err: &Error) -> bool {
    match err {
        Error::Http { status, .. } => *status <= 199 || *status == 429 || *status >= 500,
        Error::Decode { .. } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CategoryFilter, ClassId, Error};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub(crate) async fn test_client(server: &MockServer) -> Client {
        Client::builder()
            .api_key("test-key")
            .base_url(server.uri())
            .build()
            .unwrap()
    }

    #[test]
    fn empty_key_is_invalid_query() {
        let err = Client::new("").unwrap_err();
        assert!(matches!(
            err,
            Error::InvalidQuery {
                message: "empty api key"
            }
        ));
    }

    #[test]
    fn debug_hides_key() {
        let c = Client::new("super-secret-key-value").unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains("super-secret-key-value"), "{shown}");
    }

    #[tokio::test]
    async fn categories_unwraps_data() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .and(query_param("gameId", "432"))
            .and(header("x-api-key", "test-key"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":[{"id":6,"gameId":432,"name":"Mods","slug":"mc-mods","isClass":true}]}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let cats = test_client(&server)
            .await
            .categories(CategoryFilter::All)
            .await
            .unwrap();
        assert_eq!(cats.len(), 1);
        assert_eq!(cats[0].id, 6);
        assert_eq!(cats[0].name, "Mods");
    }

    #[tokio::test]
    async fn categories_classes_only_and_children() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .and(query_param("classesOnly", "true"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .and(query_param("classId", "6"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let c = test_client(&server).await;
        c.categories(CategoryFilter::ClassesOnly).await.unwrap();
        c.categories(CategoryFilter::ChildrenOf(ClassId::MODS))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn retries_503_then_ok() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json"),
            )
            .mount(&server)
            .await;
        test_client(&server)
            .await
            .categories(CategoryFilter::All)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn no_retry_on_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/categories"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let err = test_client(&server)
            .await
            .categories(CategoryFilter::All)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Http { status: 404, .. }));
    }

    #[tokio::test]
    async fn search_encodes_json_array_query_params() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/mods/search"))
            .and(query_param("gameId", "432"))
            .and(query_param("classId", "6"))
            .and(query_param("gameVersions", r#"["1.20.1"]"#))
            .and(query_param("modLoaderTypes", "[1]"))
            .and(query_param("categoryIds", "[421]"))
            .and(query_param("searchFilter", "jei"))
            .and(query_param("sortField", "2"))
            .and(query_param("sortOrder", "desc"))
            .and(query_param("index", "0"))
            .and(query_param("pageSize", "20"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":[],"pagination":{"index":0,"pageSize":20,"resultCount":0,"totalCount":0}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let page = test_client(&server)
            .await
            .search(
                &crate::SearchQuery::new(ClassId::MODS)
                    .search("jei")
                    .game_version("1.20.1")
                    .loader(crate::ModLoaderType::Forge)
                    .category(421),
            )
            .await
            .unwrap();
        assert!(page.data.is_empty());
        assert_eq!(page.pagination.page_size, 20);
    }

    #[tokio::test]
    async fn search_rejects_page_size_51() {
        let server = MockServer::start().await;
        let err = test_client(&server)
            .await
            .search(&crate::SearchQuery::new(ClassId::MODS).page_size(51))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidQuery { .. }));
    }

    #[tokio::test]
    async fn search_rejects_eleven_categories() {
        let server = MockServer::start().await;
        let err = test_client(&server)
            .await
            .search(
                &crate::SearchQuery::new(ClassId::MODS).categories((1..=11).collect::<Vec<_>>()),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, Error::InvalidQuery { .. }));
    }

    #[tokio::test]
    async fn get_mod_404_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/mods/238222"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = test_client(&server).await.get_mod(238222).await.unwrap_err();
        assert!(matches!(
            err,
            Error::NotFound {
                kind: crate::ResourceKind::Mod,
                id: 238222
            }
        ));
    }

    #[tokio::test]
    async fn get_mod_ok() {
        let server = MockServer::start().await;
        let body = format!(
            r#"{{"data":{}}}"#,
            include_str!("../tests/fixtures/mod_jei.json")
        );
        Mock::given(method("GET"))
            .and(path("/v2/mods/238222"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .mount(&server)
            .await;
        let m = test_client(&server).await.get_mod(238222).await.unwrap();
        assert_eq!(m.slug, "jei");
    }

    #[tokio::test]
    async fn get_mods_empty_skips_http() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let out = test_client(&server).await.get_mods(&[]).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn description_unwraps_html() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/238222/description"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":"<p>html</p>"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let html = test_client(&server).await.description(238222).await.unwrap();
        assert_eq!(html, "<p>html</p>");
    }

    #[tokio::test]
    async fn description_404_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/1/description"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = test_client(&server).await.description(1).await.unwrap_err();
        assert!(matches!(
            err,
            Error::NotFound {
                kind: crate::ResourceKind::Mod,
                id: 1
            }
        ));
    }

    #[tokio::test]
    async fn get_file_404_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/250898/files/5754631"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let err = test_client(&server)
            .await
            .get_file(250898, 5754631)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            Error::NotFound {
                kind: crate::ResourceKind::File,
                id: 5754631
            }
        ));
    }

    #[tokio::test]
    async fn files_sends_single_loader_and_page() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/238222/files"))
            .and(query_param("gameVersion", "1.20.1"))
            .and(query_param("modLoaderType", "1"))
            .and(query_param("index", "0"))
            .and(query_param("pageSize", "20"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":[],"pagination":{"index":0,"pageSize":20,"resultCount":0,"totalCount":0}}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        test_client(&server)
            .await
            .files(
                238222,
                &crate::FileFilter {
                    game_version: Some("1.20.1".into()),
                    loader: Some(crate::ModLoaderType::Forge),
                    ..crate::FileFilter::default()
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn get_files_chunks_101_ids() {
        use std::sync::{Arc, Mutex};
        let server = MockServer::start().await;
        let sizes = Arc::new(Mutex::new(Vec::new()));
        Mock::given(method("POST"))
            .and(path("/v1/mods/files"))
            .respond_with(CaptureSizes(sizes.clone()))
            .expect(2)
            .mount(&server)
            .await;
        let ids: Vec<u32> = (1..=101).collect();
        test_client(&server).await.get_files(&ids).await.unwrap();
        let got = sizes.lock().unwrap().clone();
        assert_eq!(got, vec![100, 1]);
    }

    #[tokio::test]
    async fn changelog_unwraps_html() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/mods/1/files/2/changelog"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                br#"{"data":"<p>notes</p>"}"#,
                "application/json",
            ))
            .mount(&server)
            .await;
        let html = test_client(&server).await.changelog(1, 2).await.unwrap();
        assert_eq!(html, "<p>notes</p>");
    }

    #[tokio::test]
    async fn get_files_empty_skips_http() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        assert!(
            test_client(&server)
                .await
                .get_files(&[])
                .await
                .unwrap()
                .is_empty()
        );
    }

    struct CaptureSizes(std::sync::Arc<std::sync::Mutex<Vec<usize>>>);
    impl wiremock::Respond for CaptureSizes {
        fn respond(&self, req: &wiremock::Request) -> ResponseTemplate {
            let v: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            self.0
                .lock()
                .unwrap()
                .push(v["fileIds"].as_array().unwrap().len());
            ResponseTemplate::new(200).set_body_raw(br#"{"data":[]}"#, "application/json")
        }
    }
}
