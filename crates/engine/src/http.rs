use crate::error::EngineError;
use serde::de::DeserializeOwned;
use sha1::{Digest, Sha1};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

pub struct HttpFiles {
    pub client: reqwest::Client,
}

impl HttpFiles {
    pub fn new() -> Result<Self, EngineError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("kmine/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|err| {
                EngineError::io(
                    PathBuf::from("http-client"),
                    std::io::Error::other(err.to_string()),
                )
            })?;
        Ok(Self { client })
    }

    pub async fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        cancel: &CancellationToken,
    ) -> Result<T, EngineError> {
        let response = self.send_ok(url, cancel).await?;
        tokio::select! {
            _ = cancel.cancelled() => Err(EngineError::Cancelled),
            result = response.json::<T>() => result.map_err(|err| reqwest_error(url, err)),
        }
    }

    pub async fn download_sha1(
        &self,
        url: &str,
        dest: &Path,
        expected_sha1: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(), EngineError> {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        if cache_hit(dest, expected_sha1)? {
            return Ok(());
        }
        if dest.exists() {
            std::fs::remove_file(dest).map_err(|e| EngineError::io(dest, e))?;
        }
        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| EngineError::io(parent, e))?;
            }
        }

        let part = dest.with_extension("part");
        let result = self
            .download_to_part(url, dest, &part, expected_sha1, cancel)
            .await;
        if result.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        result
    }

    async fn send_ok(
        &self,
        url: &str,
        cancel: &CancellationToken,
    ) -> Result<reqwest::Response, EngineError> {
        tokio::select! {
            _ = cancel.cancelled() => Err(EngineError::Cancelled),
            result = self.client.get(url).send() => {
                let response = result.map_err(|err| reqwest_error(url, err))?;
                let status = response.status();
                if status.is_success() {
                    Ok(response)
                } else {
                    Err(EngineError::Http {
                        url: url.to_string(),
                        status: status.as_u16(),
                    })
                }
            }
        }
    }

    async fn download_to_part(
        &self,
        url: &str,
        dest: &Path,
        part: &Path,
        expected_sha1: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(), EngineError> {
        let mut response = self.send_ok(url, cancel).await?;
        let mut file = std::fs::File::create(part).map_err(|e| EngineError::io(part, e))?;
        let mut hasher = Sha1::new();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return Err(EngineError::Cancelled),
                chunk = response.chunk() => {
                    match chunk {
                        Ok(Some(bytes)) => {
                            hasher.update(&bytes);
                            file.write_all(&bytes).map_err(|e| EngineError::io(part, e))?;
                        }
                        Ok(None) => break,
                        Err(err) => return Err(reqwest_error(url, err)),
                    }
                }
            }
        }
        file.flush().map_err(|e| EngineError::io(part, e))?;
        drop(file);

        let actual = hex::encode(hasher.finalize());
        if let Some(expected) = expected_sha1 {
            let expected = expected.to_ascii_lowercase();
            if actual != expected {
                return Err(EngineError::ChecksumMismatch {
                    path: dest.to_path_buf(),
                    expected,
                    actual,
                });
            }
        }
        std::fs::rename(part, dest).map_err(|e| EngineError::io(dest, e))?;
        Ok(())
    }
}

fn cache_hit(dest: &Path, expected_sha1: Option<&str>) -> Result<bool, EngineError> {
    let meta = match std::fs::metadata(dest) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(EngineError::io(dest, err)),
    };
    if !meta.is_file() || meta.len() == 0 {
        return Ok(false);
    }
    match expected_sha1 {
        None => Ok(true),
        Some(expected) => Ok(hash_file(dest)? == expected.to_ascii_lowercase()),
    }
}

fn hash_file(path: &Path) -> Result<String, EngineError> {
    let bytes = std::fs::read(path).map_err(|e| EngineError::io(path, e))?;
    Ok(hex::encode(Sha1::digest(bytes)))
}

fn reqwest_error(url: &str, err: reqwest::Error) -> EngineError {
    if let Some(status) = err.status() {
        EngineError::Http {
            url: url.to_string(),
            status: status.as_u16(),
        }
    } else {
        EngineError::io(PathBuf::from(url), std::io::Error::other(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::HttpFiles;
    use crate::error::EngineError;
    use sha1::{Digest, Sha1};
    use std::path::Path;
    use tokio_util::sync::CancellationToken;

    fn sha1_hex(bytes: &[u8]) -> String {
        hex::encode(Sha1::digest(bytes))
    }

    #[tokio::test]
    async fn downloads_and_verifies_sha1() {
        let server = wiremock::MockServer::start().await;
        let body = b"abc";
        let hash = sha1_hex(body);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(body.as_slice(), "text/plain"),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        let http = HttpFiles::new().unwrap();
        http.download_sha1(
            &format!("{}/f", server.uri()),
            &dest,
            Some(&hash),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn cache_hit_skips_network() {
        let server = wiremock::MockServer::start().await;
        let body = b"abc";
        let hash = sha1_hex(body);
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        std::fs::write(&dest, body).unwrap();
        let http = HttpFiles::new().unwrap();
        http.download_sha1(
            &format!("{}/f", server.uri()),
            &dest,
            Some(&hash),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn bad_sha1_errors() {
        let server = wiremock::MockServer::start().await;
        let body = b"abc";
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw(body.as_slice(), "text/plain"),
            )
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("f.bin");
        let http = HttpFiles::new().unwrap();
        let err = http
            .download_sha1(
                &format!("{}/f", server.uri()),
                &dest,
                Some("deadbeef"),
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, EngineError::ChecksumMismatch { .. }));
    }

    #[tokio::test]
    async fn cancel_stops_download() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = HttpFiles::new()
            .unwrap()
            .download_sha1("http://127.0.0.1:1/", Path::new("/tmp/x"), None, &cancel)
            .await
            .unwrap_err();
        assert!(matches!(err, EngineError::Cancelled));
    }
}
