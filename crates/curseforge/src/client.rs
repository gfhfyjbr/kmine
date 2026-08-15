use crate::Error;
use crate::search::CategoryFilter;
use crate::types::{Category, DEFAULT_BASE_URL, MINECRAFT_GAME_ID, Page, Pagination};
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

    #[allow(dead_code)] // reserved for paginated endpoints
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

    #[allow(dead_code)] // reserved for POST batch endpoints
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
}
