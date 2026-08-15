use super::constants::{
    MC_LOGIN_URL, MC_PROFILE_URL, TOKEN_URL, XBOX_AUTH_URL, XSTS_URL, client_id, redirect_url,
};
use crate::error::EngineError;
use crate::http::HttpFiles;
use crate::ids::AccountId;
use crate::store::Store;
use crate::types::{AccountRecord, AccountSummary};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

const VALID_FOR: Duration = Duration::seconds(60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub token: String,
    pub expiry: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Xsts {
    pub token: String,
    pub expiry: DateTime<Utc>,
    pub userhash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountSecrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msa_refresh: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msa_access: Option<Token>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xbl: Option<Token>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xsts: Option<Xsts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mc_access: Option<Token>,
}

#[derive(Debug, Clone)]
pub struct AuthEndpoints {
    pub token_url: String,
    pub xbox_url: String,
    pub xsts_url: String,
    pub mc_login_url: String,
    pub profile_url: String,
}

impl AuthEndpoints {
    pub fn production() -> Self {
        Self {
            token_url: TOKEN_URL.to_string(),
            xbox_url: XBOX_AUTH_URL.to_string(),
            xsts_url: XSTS_URL.to_string(),
            mc_login_url: MC_LOGIN_URL.to_string(),
            profile_url: MC_PROFILE_URL.to_string(),
        }
    }
}

pub fn secret_id(uuid: AccountId) -> String {
    format!("account/{}", uuid.as_hyphenated())
}

pub enum TokenPersist {
    Unchanged,
    Save(AccountSecrets),
    Delete,
}

pub async fn ensure_mc_token(
    http: &HttpFiles,
    store: &Store,
    key: &[u8; 32],
    account: AccountId,
    now: DateTime<Utc>,
    endpoints: &AuthEndpoints,
) -> Result<String, EngineError> {
    let sid = secret_id(account);
    let secrets = load_secrets(store, key, &sid)?;
    let (token, persist) = ensure_mc_token_owned(http, secrets, now, endpoints, |s| {
        save_secrets(store, key, &sid, s)
    })
    .await?;
    match persist {
        TokenPersist::Unchanged => Ok(token),
        TokenPersist::Save(secrets) => {
            save_secrets(store, key, &sid, &secrets)?;
            Ok(token)
        }
        TokenPersist::Delete => {
            store.delete_secret(&sid)?;
            Err(EngineError::AuthExpired)
        }
    }
}

pub async fn ensure_mc_token_owned(
    http: &HttpFiles,
    mut secrets: AccountSecrets,
    now: DateTime<Utc>,
    endpoints: &AuthEndpoints,
    persist: impl Fn(&AccountSecrets) -> Result<(), EngineError>,
) -> Result<(String, TokenPersist), EngineError> {
    if let Some(mc) = secrets.mc_access.as_ref() {
        if still_valid(mc.expiry, now) {
            return Ok((mc.token.clone(), TokenPersist::Unchanged));
        }
    }

    if secrets
        .xsts
        .as_ref()
        .is_some_and(|t| still_valid(t.expiry, now))
    {
        minecraft_login(http, endpoints, &mut secrets, now).await?;
        let token = take_mc(&secrets)?;
        return Ok((token, TokenPersist::Save(secrets)));
    }

    if secrets
        .xbl
        .as_ref()
        .is_some_and(|t| still_valid(t.expiry, now))
    {
        xsts_auth(http, endpoints, &mut secrets, now).await?;
        minecraft_login(http, endpoints, &mut secrets, now).await?;
        let token = take_mc(&secrets)?;
        return Ok((token, TokenPersist::Save(secrets)));
    }

    if secrets
        .msa_access
        .as_ref()
        .is_some_and(|t| still_valid(t.expiry, now))
    {
        xbox_auth(http, endpoints, &mut secrets, now).await?;
        xsts_auth(http, endpoints, &mut secrets, now).await?;
        minecraft_login(http, endpoints, &mut secrets, now).await?;
        let token = take_mc(&secrets)?;
        return Ok((token, TokenPersist::Save(secrets)));
    }

    let Some(refresh) = secrets.msa_refresh.clone().filter(|s| !s.is_empty()) else {
        return Err(EngineError::AuthExpired);
    };

    match refresh_msa_token(http, endpoints, &refresh, now).await {
        Ok((new_refresh, access)) => {
            secrets.msa_refresh = Some(new_refresh);
            secrets.msa_access = Some(access);
            persist(&secrets)?;
        }
        Err(EngineError::AuthExpired) => {
            return Ok((String::new(), TokenPersist::Delete));
        }
        Err(err) => return Err(err),
    }

    xbox_auth(http, endpoints, &mut secrets, now).await?;
    xsts_auth(http, endpoints, &mut secrets, now).await?;
    minecraft_login(http, endpoints, &mut secrets, now).await?;
    let token = take_mc(&secrets)?;
    Ok((token, TokenPersist::Save(secrets)))
}

pub async fn login_with_code(
    http: &HttpFiles,
    store: &Store,
    key: &[u8; 32],
    code: &str,
    pkce_verifier: &str,
    endpoints: &AuthEndpoints,
) -> Result<AccountSummary, EngineError> {
    let (record, secrets) = complete_login(http, code, pkce_verifier, endpoints).await?;
    persist_login(store, key, &record, &secrets)
}

pub(crate) async fn complete_login(
    http: &HttpFiles,
    code: &str,
    pkce_verifier: &str,
    endpoints: &AuthEndpoints,
) -> Result<(AccountRecord, AccountSecrets), EngineError> {
    let now = Utc::now();
    let (refresh, access) = exchange_code(http, endpoints, code, pkce_verifier, now).await?;
    let mut secrets = AccountSecrets {
        msa_refresh: Some(refresh),
        msa_access: Some(access),
        xbl: None,
        xsts: None,
        mc_access: None,
    };
    xbox_auth(http, endpoints, &mut secrets, now).await?;
    xsts_auth(http, endpoints, &mut secrets, now).await?;
    minecraft_login(http, endpoints, &mut secrets, now).await?;
    let mc = secrets.mc_access.as_ref().ok_or(EngineError::AuthFailed {
        message: "minecraft token missing after login".into(),
    })?;
    let profile = fetch_profile(http, endpoints, &mc.token).await?;
    let uuid = parse_profile_uuid(&profile.id)?;
    let ts = now.timestamp_millis();
    Ok((
        AccountRecord {
            uuid,
            username: profile.name,
            added_at: ts,
            last_used_at: Some(ts),
        },
        secrets,
    ))
}

pub(crate) fn persist_login(
    store: &Store,
    key: &[u8; 32],
    record: &AccountRecord,
    secrets: &AccountSecrets,
) -> Result<AccountSummary, EngineError> {
    store.upsert_account(record)?;
    save_secrets(store, key, &secret_id(record.uuid), secrets)?;
    store.set_selected_account(Some(record.uuid))?;
    Ok(AccountSummary {
        uuid: record.uuid,
        username: record.username.clone(),
        selected: true,
    })
}

fn still_valid(expiry: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    expiry > now + VALID_FOR
}

fn take_mc(secrets: &AccountSecrets) -> Result<String, EngineError> {
    secrets
        .mc_access
        .as_ref()
        .map(|t| t.token.clone())
        .ok_or(EngineError::AuthFailed {
            message: "minecraft token missing after refresh".into(),
        })
}

fn load_secrets(store: &Store, key: &[u8; 32], id: &str) -> Result<AccountSecrets, EngineError> {
    let raw = match store.get_secret(key, id) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Err(EngineError::AuthExpired),
        Err(EngineError::Crypto) => return Err(EngineError::AuthExpired),
        Err(err) => return Err(err),
    };
    serde_json::from_slice(&raw).map_err(|_| EngineError::AuthExpired)
}

fn save_secrets(
    store: &Store,
    key: &[u8; 32],
    id: &str,
    secrets: &AccountSecrets,
) -> Result<(), EngineError> {
    let bytes = serde_json::to_vec(secrets).map_err(|_| EngineError::AuthFailed {
        message: "failed to encode secrets".into(),
    })?;
    store.put_secret(key, id, &bytes)
}

async fn refresh_msa_token(
    http: &HttpFiles,
    endpoints: &AuthEndpoints,
    refresh: &str,
    now: DateTime<Utc>,
) -> Result<(String, Token), EngineError> {
    let id = client_id();
    let params = [
        ("client_id", id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh),
        ("scope", "XboxLive.signin XboxLive.offline_access"),
    ];
    let token = post_msa_token(http, &endpoints.token_url, &params).await?;
    Ok(msa_pair(token, refresh, now)?)
}

async fn exchange_code(
    http: &HttpFiles,
    endpoints: &AuthEndpoints,
    code: &str,
    pkce_verifier: &str,
    now: DateTime<Utc>,
) -> Result<(String, Token), EngineError> {
    let id = client_id();
    let redirect = redirect_url();
    let params = [
        ("client_id", id.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect.as_str()),
        ("code_verifier", pkce_verifier),
    ];
    let token = post_msa_token(http, &endpoints.token_url, &params).await?;
    let refresh = token.refresh_token.clone().ok_or(EngineError::AuthFailed {
        message: "token response missing refresh_token".into(),
    })?;
    Ok(msa_pair(token, &refresh, now)?)
}

fn msa_pair(
    token: MsTokenResponse,
    fallback_refresh: &str,
    now: DateTime<Utc>,
) -> Result<(String, Token), EngineError> {
    let refresh = token
        .refresh_token
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_refresh.to_string());
    Ok((
        refresh,
        Token {
            token: token.access_token,
            expiry: now + Duration::seconds(token.expires_in.max(0)),
        },
    ))
}

async fn xbox_auth(
    http: &HttpFiles,
    endpoints: &AuthEndpoints,
    secrets: &mut AccountSecrets,
    _now: DateTime<Utc>,
) -> Result<(), EngineError> {
    let msa = secrets.msa_access.as_ref().ok_or(EngineError::AuthFailed {
        message: "missing msa access token".into(),
    })?;
    let rps = format!("d={}", msa.token);
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": rps,
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT",
    });
    let xb = post_xbox(http, &endpoints.xbox_url, &body).await?;
    secrets.xbl = Some(Token {
        token: xb.token,
        expiry: parse_expiry(&xb.not_after)?,
    });
    Ok(())
}

async fn xsts_auth(
    http: &HttpFiles,
    endpoints: &AuthEndpoints,
    secrets: &mut AccountSecrets,
    _now: DateTime<Utc>,
) -> Result<(), EngineError> {
    let xbl = secrets.xbl.as_ref().ok_or(EngineError::AuthFailed {
        message: "missing xbox token".into(),
    })?;
    let body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl.token],
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT",
    });
    let xb = post_xbox(http, &endpoints.xsts_url, &body).await?;
    let userhash = xb
        .display_claims
        .xui
        .into_iter()
        .next()
        .map(|x| x.uhs)
        .ok_or(EngineError::AuthFailed {
            message: "xsts response missing userhash".into(),
        })?;
    secrets.xsts = Some(Xsts {
        token: xb.token,
        expiry: parse_expiry(&xb.not_after)?,
        userhash,
    });
    Ok(())
}

async fn minecraft_login(
    http: &HttpFiles,
    endpoints: &AuthEndpoints,
    secrets: &mut AccountSecrets,
    now: DateTime<Utc>,
) -> Result<(), EngineError> {
    let xsts = secrets.xsts.as_ref().ok_or(EngineError::AuthFailed {
        message: "missing xsts token".into(),
    })?;
    let identity = format!("XBL3.0 x={};{}", xsts.userhash, xsts.token);
    let body = serde_json::json!({ "identityToken": identity });
    let resp = http
        .client
        .post(&endpoints.mc_login_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| io_http(&endpoints.mc_login_url, e))?;
    let status = resp.status();
    if status.as_u16() == 403 {
        return Err(EngineError::AuthFailed {
            message: format!(
                "Minecraft blocked this Azure app (403). Request API access at https://aka.ms/mce-reviewappid with client id {}",
                client_id()
            ),
        });
    }
    if !status.is_success() {
        return Err(EngineError::Http {
            url: endpoints.mc_login_url.clone(),
            status: status.as_u16(),
        });
    }
    let parsed: McLoginResponse = resp
        .json()
        .await
        .map_err(|e| io_http(&endpoints.mc_login_url, e))?;
    secrets.mc_access = Some(Token {
        token: parsed.access_token,
        expiry: now + Duration::seconds(parsed.expires_in.max(0)),
    });
    Ok(())
}

async fn fetch_profile(
    http: &HttpFiles,
    endpoints: &AuthEndpoints,
    mc_token: &str,
) -> Result<McProfile, EngineError> {
    let resp = http
        .client
        .get(&endpoints.profile_url)
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| io_http(&endpoints.profile_url, e))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err(EngineError::MinecraftNotOwned);
    }
    if !status.is_success() {
        return Err(EngineError::Http {
            url: endpoints.profile_url.clone(),
            status: status.as_u16(),
        });
    }
    resp.json()
        .await
        .map_err(|e| io_http(&endpoints.profile_url, e))
}

async fn post_msa_token(
    http: &HttpFiles,
    url: &str,
    params: &[(&str, &str)],
) -> Result<MsTokenResponse, EngineError> {
    let resp = http
        .client
        .post(url)
        .form(params)
        .send()
        .await
        .map_err(|e| io_http(url, e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| io_http(url, e))?;
    if let Ok(err) = serde_json::from_str::<OAuthErrorBody>(&text) {
        if err.error == "invalid_grant" {
            return Err(EngineError::AuthExpired);
        }
        if !status.is_success() {
            return Err(EngineError::AuthFailed {
                message: format!("token endpoint: {}", err.error),
            });
        }
    }
    if !status.is_success() {
        return Err(EngineError::Http {
            url: url.to_string(),
            status: status.as_u16(),
        });
    }
    serde_json::from_str(&text).map_err(|_| EngineError::AuthFailed {
        message: "token endpoint returned invalid json".into(),
    })
}

async fn post_xbox(
    http: &HttpFiles,
    url: &str,
    body: &serde_json::Value,
) -> Result<XboxResponse, EngineError> {
    let resp = http
        .client
        .post(url)
        .header("Accept", "application/json")
        .header("x-xbl-contract-version", "1")
        .json(body)
        .send()
        .await
        .map_err(|e| io_http(url, e))?;
    let status = resp.status();
    if !status.is_success() {
        if let Ok(text) = resp.text().await {
            if let Ok(err) = serde_json::from_str::<XboxErrorBody>(&text) {
                if let Some(xerr) = err.xerr {
                    return Err(EngineError::AuthFailed {
                        message: format!("xbox error {xerr}"),
                    });
                }
            }
        }
        return Err(EngineError::Http {
            url: url.to_string(),
            status: status.as_u16(),
        });
    }
    resp.json().await.map_err(|e| io_http(url, e))
}

fn parse_expiry(raw: &str) -> Result<DateTime<Utc>, EngineError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| EngineError::AuthFailed {
            message: "invalid xbox token expiry".into(),
        })
}

fn parse_profile_uuid(id: &str) -> Result<AccountId, EngineError> {
    uuid::Uuid::try_parse(id)
        .map(AccountId)
        .map_err(|_| EngineError::AuthFailed {
            message: "profile uuid is invalid".into(),
        })
}

fn io_http(url: &str, err: reqwest::Error) -> EngineError {
    if let Some(status) = err.status() {
        EngineError::Http {
            url: url.to_string(),
            status: status.as_u16(),
        }
    } else {
        EngineError::io(url, std::io::Error::other(err.to_string()))
    }
}

#[derive(Deserialize)]
struct MsTokenResponse {
    access_token: String,
    expires_in: i64,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct OAuthErrorBody {
    error: String,
}

#[derive(Deserialize)]
struct XboxResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "NotAfter")]
    not_after: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XboxDisplayClaims,
}

#[derive(Deserialize)]
struct XboxDisplayClaims {
    #[serde(default)]
    xui: Vec<XboxXui>,
}

#[derive(Deserialize)]
struct XboxXui {
    uhs: String,
}

#[derive(Deserialize)]
struct XboxErrorBody {
    #[serde(rename = "XErr")]
    xerr: Option<u64>,
}

#[derive(Deserialize)]
struct McLoginResponse {
    access_token: String,
    expires_in: i64,
}

#[derive(Deserialize)]
struct McProfile {
    id: String,
    name: String,
}
