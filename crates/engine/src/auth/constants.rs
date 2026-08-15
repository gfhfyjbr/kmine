pub const CLIENT_ID: &str = "426346bc-d0fc-4222-983c-1a6d20e6342d";
pub const AUTH_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const REDIRECT_URL: &str = "http://127.0.0.1:47821/auth";
pub const BIND: &str = "127.0.0.1:47821";
pub const XBOX_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
pub const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// `KMINE_MSA_CLIENT_ID` overrides the baked-in Azure app for local login tests.
pub fn client_id() -> String {
    env_override("KMINE_MSA_CLIENT_ID").unwrap_or_else(|| CLIENT_ID.to_string())
}

/// `KMINE_MSA_REDIRECT_URL` must match the Azure app that issued `client_id()`.
pub fn redirect_url() -> String {
    env_override("KMINE_MSA_REDIRECT_URL").unwrap_or_else(|| REDIRECT_URL.to_string())
}

/// `KMINE_MSA_BIND` or host:port taken from `redirect_url()` (`localhost` → `127.0.0.1`).
pub fn bind_addr() -> String {
    if let Some(bind) = env_override("KMINE_MSA_BIND") {
        return bind;
    }
    bind_from_redirect(&redirect_url()).unwrap_or_else(|| BIND.to_string())
}

fn env_override(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn bind_from_redirect(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port().unwrap_or(80);
    let host = if host.eq_ignore_ascii_case("localhost") {
        "127.0.0.1"
    } else {
        host
    };
    Some(format!("{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::bind_from_redirect;

    #[test]
    fn localhost_redirect_binds_loopback_port() {
        assert_eq!(
            bind_from_redirect("http://localhost:3160/auth").as_deref(),
            Some("127.0.0.1:3160")
        );
    }
}
