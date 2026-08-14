use super::constants::{AUTH_URL, BIND, CLIENT_ID, REDIRECT_URL};
use crate::error::EngineError;
use oauth2::basic::BasicClient;
use oauth2::{AuthUrl, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const ACCEPT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const ACCEPT_POLL: Duration = Duration::from_millis(100);

pub struct AuthorizeRequest {
    pub url: String,
    pub state: String,
    pub pkce_verifier: String,
}

#[derive(Debug)]
pub struct AuthCallback {
    pub code: String,
    pub state: String,
}

pub fn authorize_request() -> Result<AuthorizeRequest, EngineError> {
    let client = BasicClient::new(ClientId::new(CLIENT_ID.to_string()))
        .set_auth_uri(parse_auth_url(AUTH_URL)?)
        .set_redirect_uri(parse_redirect_url(REDIRECT_URL)?);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, csrf) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("XboxLive.signin".into()))
        .add_scope(Scope::new("XboxLive.offline_access".into()))
        .set_pkce_challenge(challenge)
        .add_extra_param("prompt", "select_account")
        .url();
    Ok(AuthorizeRequest {
        url: url.to_string(),
        state: csrf.secret().clone(),
        pkce_verifier: verifier.secret().clone(),
    })
}

pub fn wait_for_callback(
    listener: TcpListener,
    cancel: &CancellationToken,
) -> Result<AuthCallback, EngineError> {
    wait_for_callback_deadline(listener, cancel, Instant::now() + ACCEPT_TIMEOUT)
}

pub(crate) fn wait_for_callback_deadline(
    listener: TcpListener,
    cancel: &CancellationToken,
    deadline: Instant,
) -> Result<AuthCallback, EngineError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| EngineError::io(BIND, e))?;
    loop {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(EngineError::AuthFailed {
                message: "oauth callback timed out".into(),
            });
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                if let Some(callback) = handle_connection(stream)? {
                    return Ok(callback);
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::Interrupted =>
            {
                std::thread::sleep(ACCEPT_POLL);
            }
            Err(err) => return Err(EngineError::io(BIND, err)),
        }
    }
}

fn handle_connection(mut stream: TcpStream) -> Result<Option<AuthCallback>, EngineError> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
    let (method, target) = read_request(&mut stream)?;
    let parsed = match url::Url::parse(&format!("http://{BIND}{target}")) {
        Ok(url) => url,
        Err(_) => {
            write_response(&mut stream, 400, "text/plain", "Bad Request");
            return Err(EngineError::AuthFailed {
                message: "malformed oauth callback".into(),
            });
        }
    };
    if parsed.path() != "/auth" {
        write_response(&mut stream, 404, "text/plain", "Not Found");
        return Ok(None);
    }
    write_response(
        &mut stream,
        200,
        "text/html; charset=utf-8",
        "<!DOCTYPE html><html><body>You can close this tab.</body></html>",
    );
    if method != "GET" {
        return Err(EngineError::AuthFailed {
            message: "oauth callback must be GET".into(),
        });
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "code" if code.is_none() => code = Some(value.into_owned()),
            "state" if state.is_none() => state = Some(value.into_owned()),
            "error" if error.is_none() => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Err(EngineError::AuthFailed {
            message: format!("oauth {error}"),
        });
    }
    match (code, state) {
        (Some(code), Some(state)) => Ok(Some(AuthCallback { code, state })),
        _ => Err(EngineError::AuthFailed {
            message: "oauth callback missing code or state".into(),
        }),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<(String, String), EngineError> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = stream
            .read(&mut tmp)
            .map_err(|e| EngineError::io(BIND, e))?;
        if n == 0 {
            return Err(EngineError::AuthFailed {
                message: "empty auth callback".into(),
            });
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 16 * 1024 {
            return Err(EngineError::AuthFailed {
                message: "auth callback too large".into(),
            });
        }
        let mut headers = [httparse::EMPTY_HEADER; 32];
        let mut req = httparse::Request::new(&mut headers);
        match req.parse(&buf) {
            Ok(httparse::Status::Complete(_)) => {
                return Ok((
                    req.method.unwrap_or("").to_string(),
                    req.path.unwrap_or("/").to_string(),
                ));
            }
            Ok(httparse::Status::Partial) => {}
            Err(_) => {
                return Err(EngineError::AuthFailed {
                    message: "malformed auth callback".into(),
                });
            }
        }
    }
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.shutdown(Shutdown::Both);
}

fn parse_auth_url(raw: &str) -> Result<AuthUrl, EngineError> {
    AuthUrl::new(raw.to_string()).map_err(|_| EngineError::AuthFailed {
        message: "invalid authorization url".into(),
    })
}

fn parse_redirect_url(raw: &str) -> Result<RedirectUrl, EngineError> {
    RedirectUrl::new(raw.to_string()).map_err(|_| EngineError::AuthFailed {
        message: "invalid redirect url".into(),
    })
}
