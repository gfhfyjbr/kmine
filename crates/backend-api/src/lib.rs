use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use kmine_curseforge::{extract_from_source, CfCoreKey};
use serde_json::json;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

const URL_REFRESH: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone)]
pub struct AppState {
    pub key: Option<CfCoreKey>,
    pub token: Option<String>,
    pub source: Option<String>,
    pub last_mtime: Option<SystemTime>,
    pub last_url_extract: Option<Instant>,
}

impl AppState {
    pub fn empty() -> Self {
        Self {
            key: None,
            token: None,
            source: None,
            last_mtime: None,
            last_url_extract: None,
        }
    }

    pub fn from_source(source: &str) -> Result<Self, String> {
        let key = extract_from_source(source).ok();
        Ok(Self {
            key,
            token: std::env::var("KMINE_BACKEND_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
            source: Some(source.to_string()),
            last_mtime: mtime_if_path(source),
            last_url_extract: source.starts_with("http").then(Instant::now),
        })
    }
}

pub fn app(state: AppState) -> Router {
    router(Arc::new(RwLock::new(state)))
}

pub fn router(state: Arc<RwLock<AppState>>) -> Router {
    Router::new()
        .route("/get_cf_api_key", get(get_cf_api_key))
        .layer(from_fn_with_state(state.clone(), require_token))
        .with_state(state)
}

pub fn reextract(state: &Arc<RwLock<AppState>>) {
    let mut st = state.write().unwrap();
    let Some(source) = st.source.clone() else {
        return;
    };
    if let Ok(key) = extract_from_source(&source) {
        st.key = Some(key);
    } else {
        st.key = None;
    }
    if is_url(&source) {
        st.last_url_extract = Some(Instant::now());
    } else {
        st.last_mtime = mtime_if_path(&source);
    }
}

fn mtime_if_path(source: &str) -> Option<SystemTime> {
    if is_url(source) {
        return None;
    }
    std::fs::metadata(source).and_then(|m| m.modified()).ok()
}

fn is_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

fn refresh_if_needed(state: &Arc<RwLock<AppState>>) {
    let mut st = state.write().unwrap();
    let Some(source) = st.source.clone() else {
        return;
    };
    if is_url(&source) {
        let stale = st
            .last_url_extract
            .map(|t| t.elapsed() >= URL_REFRESH)
            .unwrap_or(true);
        if !stale {
            return;
        }
        if let Ok(key) = extract_from_source(&source) {
            st.key = Some(key);
        }
        st.last_url_extract = Some(Instant::now());
    } else {
        let mtime = mtime_if_path(&source);
        if mtime == st.last_mtime {
            return;
        }
        if let Ok(key) = extract_from_source(&source) {
            st.key = Some(key);
        } else {
            st.key = None;
        }
        st.last_mtime = mtime;
    }
}

async fn require_token(
    State(state): State<Arc<RwLock<AppState>>>,
    request: Request,
    next: Next,
) -> Response {
    let expected = state.read().unwrap().token.clone();
    if let Some(expected) = expected {
        let authorized = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == format!("Bearer {expected}"));
        if !authorized {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "unauthorized"})),
            )
                .into_response();
        }
    }
    next.run(request).await
}

async fn get_cf_api_key(State(state): State<Arc<RwLock<AppState>>>) -> Response {
    refresh_if_needed(&state);
    let st = state.read().unwrap();
    let Some(key) = st.key.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "key unavailable"})),
        )
            .into_response();
    };
    (
        StatusCode::OK,
        Json(json!({"apiKey": key.key, "source": key.source})),
    )
        .into_response()
}
