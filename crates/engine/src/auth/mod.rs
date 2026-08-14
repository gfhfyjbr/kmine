mod constants;
pub(crate) mod oauth;
mod tokens;

pub use constants::{AUTH_URL, BIND, CLIENT_ID, REDIRECT_URL, TOKEN_URL};
pub use tokens::{
    AccountSecrets, AuthEndpoints, Token, TokenPersist, Xsts, ensure_mc_token,
    ensure_mc_token_owned, login_with_code, secret_id,
};
pub(crate) use tokens::{complete_login, persist_login};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EngineError;
    use crate::http::HttpFiles;
    use crate::ids::AccountId;
    use crate::store::{MemoryKeychain, Store};
    use crate::types::AccountSummary;
    use crate::{Engine, LauncherPaths};
    use chrono::{Duration, Utc};
    use serde_json::json;
    use std::path::Path;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn account() -> AccountId {
        AccountId(uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap())
    }

    fn dummy_endpoints() -> AuthEndpoints {
        AuthEndpoints {
            token_url: "http://127.0.0.1:1/token".into(),
            xbox_url: "http://127.0.0.1:1/xbox".into(),
            xsts_url: "http://127.0.0.1:1/xsts".into(),
            mc_login_url: "http://127.0.0.1:1/mc".into(),
            profile_url: "http://127.0.0.1:1/profile".into(),
        }
    }

    fn endpoints_for(server: &MockServer) -> AuthEndpoints {
        AuthEndpoints {
            token_url: format!("{}/token", server.uri()),
            xbox_url: format!("{}/xbox", server.uri()),
            xsts_url: format!("{}/xsts", server.uri()),
            mc_login_url: format!("{}/mc", server.uri()),
            profile_url: format!("{}/profile", server.uri()),
        }
    }

    #[test]
    fn empty_client_id_is_not_configured() {
        assert!(CLIENT_ID.is_empty());
    }

    #[tokio::test]
    async fn ensure_mc_token_uses_cached_token() {
        let kc = MemoryKeychain::new();
        let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let now = Utc::now();
        let secrets = AccountSecrets {
            msa_refresh: Some("refresh".into()),
            msa_access: None,
            xbl: None,
            xsts: None,
            mc_access: Some(Token {
                token: "cached".into(),
                expiry: now + Duration::hours(1),
            }),
        };
        store
            .put_secret(
                &key,
                &secret_id(account()),
                &serde_json::to_vec(&secrets).unwrap(),
            )
            .unwrap();
        let http = HttpFiles::new().unwrap();
        let tok = ensure_mc_token(&http, &store, &key, account(), now, &dummy_endpoints())
            .await
            .unwrap();
        assert_eq!(tok, "cached");
    }

    #[tokio::test]
    async fn invalid_grant_is_auth_expired() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": "invalid_grant",
                "error_description": "The refresh token has expired"
            })))
            .mount(&server)
            .await;

        let kc = MemoryKeychain::new();
        let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let secrets = AccountSecrets {
            msa_refresh: Some("stale-refresh".into()),
            msa_access: None,
            xbl: None,
            xsts: None,
            mc_access: None,
        };
        store
            .put_secret(
                &key,
                &secret_id(account()),
                &serde_json::to_vec(&secrets).unwrap(),
            )
            .unwrap();

        let http = HttpFiles::new().unwrap();
        let err = ensure_mc_token(
            &http,
            &store,
            &key,
            account(),
            Utc::now(),
            &endpoints_for(&server),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, EngineError::AuthExpired));
        assert!(
            store
                .get_secret(&key, &secret_id(account()))
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn start_login_without_client_id() {
        let root = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(root.path().to_path_buf());
        paths.create_dirs().unwrap();
        let kc = MemoryKeychain::new();
        let engine = Engine::open_with_keychain(paths, &kc).unwrap();
        let err = engine.start_login().await.unwrap_err();
        assert!(matches!(err, EngineError::AuthNotConfigured));
    }

    #[tokio::test]
    async fn ensure_mc_token_refreshes_from_msa_refresh() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token_type": "Bearer",
                "expires_in": 3600,
                "access_token": "msa-access",
                "refresh_token": "msa-refresh-new",
                "scope": "XboxLive.signin XboxLive.offline_access"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/xbox"))
            .respond_with(ResponseTemplate::new(200).set_body_json(xbox_body("xbl-token", "uhash")))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/xsts"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(xbox_body("xsts-token", "uhash")),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/mc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "username": "11111111-1111-1111-1111-111111111111",
                "roles": [],
                "access_token": "fresh-mc",
                "token_type": "Bearer",
                "expires_in": 86400
            })))
            .mount(&server)
            .await;

        let kc = MemoryKeychain::new();
        let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let secrets = AccountSecrets {
            msa_refresh: Some("stale-refresh".into()),
            msa_access: None,
            xbl: None,
            xsts: None,
            mc_access: None,
        };
        store
            .put_secret(
                &key,
                &secret_id(account()),
                &serde_json::to_vec(&secrets).unwrap(),
            )
            .unwrap();

        let http = HttpFiles::new().unwrap();
        let tok = ensure_mc_token(
            &http,
            &store,
            &key,
            account(),
            Utc::now(),
            &endpoints_for(&server),
        )
        .await
        .unwrap();
        assert_eq!(tok, "fresh-mc");
        let stored: AccountSecrets = serde_json::from_slice(
            &store
                .get_secret(&key, &secret_id(account()))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(stored.msa_refresh.as_deref(), Some("msa-refresh-new"));
        assert_eq!(stored.mc_access.unwrap().token, "fresh-mc");
    }

    #[tokio::test]
    async fn login_with_code_profile_404_is_not_owned() {
        let server = MockServer::start().await;
        mount_login_success(&server, 404).await;
        let kc = MemoryKeychain::new();
        let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let http = HttpFiles::new().unwrap();
        let err = login_with_code(
            &http,
            &store,
            &key,
            "code",
            "verifier",
            &endpoints_for(&server),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, EngineError::MinecraftNotOwned));
    }

    #[tokio::test]
    async fn login_with_code_upserts_account() {
        let server = MockServer::start().await;
        mount_login_success(&server, 200).await;
        let kc = MemoryKeychain::new();
        let (store, key) = Store::open(Path::new(":memory:"), &kc).unwrap();
        let http = HttpFiles::new().unwrap();
        let summary = login_with_code(
            &http,
            &store,
            &key,
            "code",
            "verifier",
            &endpoints_for(&server),
        )
        .await
        .unwrap();
        assert_eq!(
            summary,
            AccountSummary {
                uuid: account(),
                username: "Steve".into(),
                selected: true,
            }
        );
        assert_eq!(store.selected_account().unwrap(), Some(account()));
    }

    #[tokio::test]
    async fn oauth_callback_returns_code_and_state() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::task::spawn_blocking(move || super::oauth::wait_for_callback(listener));
        let url = format!("http://{addr}/auth?code=the-code&state=the-state");
        let resp = reqwest::Client::new().get(url).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        assert!(
            resp.text()
                .await
                .unwrap()
                .contains("You can close this tab.")
        );
        let got = handle.await.unwrap().unwrap();
        assert_eq!(got.code, "the-code");
        assert_eq!(got.state, "the-state");
    }

    fn xbox_body(token: &str, uhs: &str) -> serde_json::Value {
        json!({
            "IssueInstant": "2020-01-01T00:00:00.0000000Z",
            "NotAfter": "2099-01-01T00:00:00.0000000Z",
            "Token": token,
            "DisplayClaims": { "xui": [{ "uhs": uhs }] }
        })
    }

    async fn mount_login_success(server: &MockServer, profile_status: u16) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token_type": "Bearer",
                "expires_in": 3600,
                "access_token": "msa-access",
                "refresh_token": "msa-refresh",
                "scope": "XboxLive.signin XboxLive.offline_access"
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/xbox"))
            .respond_with(ResponseTemplate::new(200).set_body_json(xbox_body("xbl-token", "uhash")))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/xsts"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(xbox_body("xsts-token", "uhash")),
            )
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/mc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "username": "11111111-1111-1111-1111-111111111111",
                "roles": [],
                "access_token": "mc-access",
                "token_type": "Bearer",
                "expires_in": 86400
            })))
            .mount(server)
            .await;
        let profile = if profile_status == 200 {
            ResponseTemplate::new(200).set_body_json(json!({
                "id": "11111111111111111111111111111111",
                "name": "Steve"
            }))
        } else {
            ResponseTemplate::new(profile_status).set_body_json(json!({
                "path": "/minecraft/profile",
                "errorType": "NOT_FOUND"
            }))
        };
        Mock::given(method("GET"))
            .and(path("/profile"))
            .respond_with(profile)
            .mount(server)
            .await;
    }
}
