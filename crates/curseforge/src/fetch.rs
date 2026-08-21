use super::extract::{CfCoreKey, CfKeyError, extract_from_bytes};
use std::time::Duration;

pub const LATEST_MAC_DMG: &str = "https://curseforge.overwolf.com/downloads/curseforge-latest.dmg";

pub fn extract_from_url(url: &str) -> Result<CfCoreKey, CfKeyError> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("kmine-cf-key/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|err| CfKeyError::Http {
            url: url.to_string(),
            message: err.to_string(),
        })?;
    let response = client.get(url).send().map_err(|err| CfKeyError::Http {
        url: url.to_string(),
        message: err.to_string(),
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(CfKeyError::Http {
            url: url.to_string(),
            message: format!("status {status}"),
        });
    }
    let bytes = response.bytes().map_err(|err| CfKeyError::Http {
        url: url.to_string(),
        message: err.to_string(),
    })?;
    let mut found =
        extract_from_bytes(&bytes).ok_or_else(|| CfKeyError::NotFound(url.to_string()))?;
    found.source = format!("url:{url}/{}", found.source);
    Ok(found)
}

pub fn extract_from_source(source: &str) -> Result<CfCoreKey, CfKeyError> {
    if source.starts_with("https://") || source.starts_with("http://") {
        extract_from_url(source)
    } else {
        super::extract::extract_from_path(source)
    }
}
