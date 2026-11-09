use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::json;

use crate::{
    backend::{Backend, TaskState},
    model::{InvalidRequest, SongRequest, SongState, SongStatus},
};

#[derive(Clone)]
pub struct AppState {
    pub backend: Arc<Backend>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/songs", post(create_song))
        .route("/v1/songs/{id}", get(song_status))
        .route("/v1/songs/{id}/audio", get(song_audio))
        .with_state(state)
}

enum ApiError {
    Invalid(InvalidRequest),
    NotReady(String),
    Backend(anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Invalid(e) => (StatusCode::BAD_REQUEST, e.to_string()),
            Self::NotReady(m) => (StatusCode::CONFLICT, m),
            Self::Backend(e) => (StatusCode::BAD_GATEWAY, format!("{e:#}")),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::Backend(e)
    }
}

async fn create_song(
    State(state): State<AppState>,
    Json(req): Json<SongRequest>,
) -> Result<(StatusCode, Json<SongStatus>), ApiError> {
    req.validate().map_err(ApiError::Invalid)?;
    let id = state.backend.submit(&req).await?;
    let status = SongStatus {
        id,
        state: SongState::Running,
        error: None,
    };
    Ok((StatusCode::ACCEPTED, Json(status)))
}

async fn song_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<SongStatus>, ApiError> {
    let (state, error) = match state.backend.task(&id).await? {
        TaskState::Running => (SongState::Running, None),
        TaskState::Succeeded { .. } => (SongState::Succeeded, None),
        TaskState::Failed(reason) => (SongState::Failed, Some(reason)),
    };
    Ok(Json(SongStatus { id, state, error }))
}

async fn song_audio(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    match state.backend.task(&id).await? {
        TaskState::Succeeded { file } => {
            let upstream = state.backend.audio(&file).await?;
            let content_type = upstream.headers().get(CONTENT_TYPE).cloned();
            let bytes = upstream.bytes().await.map_err(anyhow::Error::from)?;
            let mut response = bytes.into_response();
            if let Some(ct) = content_type {
                response.headers_mut().insert(CONTENT_TYPE, ct);
            }
            Ok(response)
        }
        TaskState::Running => Err(ApiError::NotReady("song is still generating".into())),
        TaskState::Failed(reason) => Err(ApiError::NotReady(format!("song failed: {reason}"))),
    }
}
