use kmine_backend_api::{reextract, router, AppState};
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let bind = std::env::var("KMINE_BACKEND_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());

    let mut state = match std::env::var("KMINE_CF_KEY_SOURCE") {
        Ok(source) if !source.is_empty() => {
            AppState::from_source(&source).unwrap_or_else(|_| AppState::empty())
        }
        _ => AppState::empty(),
    };
    if state.token.is_none() {
        state.token = env_token();
    }

    let state = Arc::new(RwLock::new(state));
    spawn_sighup_refresh(state.clone());

    let app = router(state);
    let listener = TcpListener::bind(&bind)
        .await
        .unwrap_or_else(|err| panic!("bind {bind}: {err}"));
    // Never log the API key.
    eprintln!("kmine-backend-api listening on {bind}");
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|err| panic!("serve: {err}"));
}

fn env_token() -> Option<String> {
    std::env::var("KMINE_BACKEND_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
}

fn spawn_sighup_refresh(state: Arc<RwLock<AppState>>) {
    #[cfg(unix)]
    {
        tokio::spawn(async move {
            let mut signals =
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            while signals.recv().await.is_some() {
                reextract(&state);
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = state;
    }
}
