use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use kmine_backend_api::{app, AppState};
use std::path::PathBuf;
use tower::ServiceExt;

#[tokio::test]
async fn returns_key_from_source_file() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/key.txt");
    let state = AppState::from_source(fixture.to_str().unwrap()).unwrap();
    let app = app(state);
    let resp = app
        .oneshot(Request::get("/get_cf_api_key").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        body["apiKey"],
        "$2a$10$bL4bIL5pUWqfcO7KQtnMReakwtfHbNKh6v1uTpKlzhwoueEJQnPnm"
    );
    assert!(body["source"].as_str().unwrap().len() > 0);
}

#[tokio::test]
async fn missing_source_is_503_without_key() {
    let state = AppState::empty();
    let app = app(state);
    let resp = app
        .oneshot(Request::get("/get_cf_api_key").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("$2a$10$"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"],
        "key unavailable"
    );
}

#[tokio::test]
async fn token_required_yields_401() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/key.txt");
    let mut state = AppState::from_source(fixture.to_str().unwrap()).unwrap();
    state.token = Some("secret".into());
    let app = app(state);
    let resp = app
        .oneshot(Request::get("/get_cf_api_key").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("$2a$10$"));
}
