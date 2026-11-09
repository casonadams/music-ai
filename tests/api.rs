use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::CONTENT_TYPE},
    routing::{get, post},
};
use musicai::{
    backend::Backend,
    server::{AppState, router},
};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn fake_backend(task_status: i64, result: &str) -> String {
    let query = json!({
        "data": [{"task_id": "t1", "status": task_status, "result": result}],
        "code": 200
    });
    let app = Router::new()
        .route(
            "/release_task",
            post(|| async { Json(json!({"data": {"task_id": "t1"}, "code": 200, "error": null})) }),
        )
        .route("/query_result", post(move || async move { Json(query) }))
        .route(
            "/v1/audio",
            get(|| async { ([(CONTENT_TYPE, "audio/flac")], b"FLACDATA".to_vec()) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

fn app(backend_url: String) -> Router {
    router(AppState {
        backend: Arc::new(Backend::new(backend_url)),
    })
}

async fn call(
    app: Router,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .map(|v| v.to_str().unwrap().to_owned());
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, content_type, bytes)
}

const DONE: &str = r#"[{"file":"/v1/audio?path=x"}]"#;

#[tokio::test]
async fn creating_a_song_returns_the_backend_task_id() {
    let app = app(fake_backend(0, "").await);
    let (status, _, body) = call(app, "POST", "/v1/songs", json!({"prompt": "lofi"})).await;
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["id"], "t1");
    assert_eq!(body["status"], "running");
}

#[tokio::test]
async fn invalid_requests_are_rejected_before_reaching_the_backend() {
    let app = app("http://127.0.0.1:1".into());
    let (status, _, body) = call(
        app,
        "POST",
        "/v1/songs",
        json!({"prompt": "x", "duration": 5}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8(body).unwrap().contains("duration"));
}

#[tokio::test]
async fn status_reflects_backend_state() {
    let cases = [
        (0, "", "running", None),
        (1, DONE, "succeeded", None),
        (1, r#"[{"file":""}]"#, "failed", Some("no audio file")),
    ];
    for (code, result, expected, error_fragment) in cases {
        let app = app(fake_backend(code, result).await);
        let (status, _, body) = call(app, "GET", "/v1/songs/t1", json!(null)).await;
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], expected);
        if let Some(fragment) = error_fragment {
            assert!(body["error"].as_str().unwrap().contains(fragment));
        }
    }
}

#[tokio::test]
async fn audio_is_proxied_with_its_content_type_once_ready() {
    let app = app(fake_backend(1, DONE).await);
    let (status, content_type, body) = call(app, "GET", "/v1/songs/t1/audio", json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("audio/flac"));
    assert_eq!(body, b"FLACDATA");
}

#[tokio::test]
async fn audio_conflicts_while_generating() {
    let app = app(fake_backend(0, "").await);
    let (status, _, _) = call(app, "GET", "/v1/songs/t1/audio", json!(null)).await;
    assert_eq!(status, StatusCode::CONFLICT);
}
