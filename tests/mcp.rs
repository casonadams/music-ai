use std::{
    f32::consts::TAU,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::State,
    http::header::CONTENT_TYPE,
    routing::{get, post},
};
use musicai::{
    backend::Backend,
    library::Library,
    mcp::{
        protocol::{ToolError, ToolSet},
        tools::Tools,
    },
    server::{AppState, router},
};
use serde_json::{Value, json};

const RATE: u32 = 8_000;

fn one_second_wav() -> Vec<u8> {
    let data: Vec<u8> = (0..RATE)
        .flat_map(|n| {
            let v = (0.5 * (TAU * 440.0 * n as f32 / RATE as f32).sin() * 32767.0) as i16;
            v.to_le_bytes()
        })
        .collect();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}

#[derive(Clone, Default)]
struct FakeAce {
    submitted: Arc<Mutex<Vec<Value>>>,
}

async fn release(State(ace): State<FakeAce>, Json(body): Json<Value>) -> Json<Value> {
    let mut submitted = ace.submitted.lock().unwrap();
    submitted.push(body);
    let id = format!("t{}", submitted.len());
    Json(json!({"data": {"task_id": id}, "code": 200, "error": null}))
}

/// A stand-in ACE-Step backend that finishes every task immediately, behind the real REST router.
async fn start_server() -> (String, FakeAce) {
    let ace = FakeAce::default();
    let result = json!([{
        "file": "/v1/audio?path=x", "seed_value": "1", "dit_model": "fake-dit",
        "metas": {"bpm": 100, "keyscale": "C major"}
    }])
    .to_string();
    let backend = Router::new()
        .route("/release_task", post(release))
        .route(
            "/query_result",
            post(move || async move {
                Json(json!({"code": 200, "data": [{"status": 1, "result": result}]}))
            }),
        )
        .route(
            "/v1/model_inventory",
            get(|| async {
                Json(json!({"code": 200, "data": {
                    "models": [{"name": "fake-dit", "is_default": true, "is_loaded": false}],
                    "default_model": "fake-dit", "lm_models": [], "loaded_lm_model": null}}))
            }),
        )
        .route(
            "/v1/audio",
            get(|| async { ([(CONTENT_TYPE, "audio/wav")], one_second_wav()) }),
        )
        .with_state(ace.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend_url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, backend).await.unwrap() });

    let app = router(AppState {
        backend: Arc::new(Backend::new(backend_url)),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, ace)
}

fn scratch_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("musicai-mcp-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn call(tools: &Tools, name: &str, args: Value) -> Value {
    tools.call(name, args).await.unwrap()
}

async fn call_err(tools: &Tools, name: &str, args: Value) -> String {
    match tools.call(name, args).await {
        Err(ToolError::Failed(message)) => message,
        other => panic!("expected a tool failure, got {other:?}"),
    }
}

async fn finished(tools: &Tools, job: &Value) -> Value {
    let done = call(
        tools,
        "get_job",
        json!({"job_id": job["job_id"], "wait_seconds": 20}),
    )
    .await;
    assert_eq!(done["status"], "succeeded", "{done}");
    done["song"].clone()
}

fn file_exists(song: &Value) -> bool {
    Path::new(song["path"].as_str().unwrap()).is_file()
}

#[tokio::test]
async fn an_agent_can_write_generate_review_and_vary_a_song() {
    let (server, ace) = start_server().await;
    let root = scratch_dir("flow");
    let library = Library {
        songs: root.join("songs"),
        styles: root.join("styles"),
        lyrics: root.join("lyrics"),
    };
    let checkpoints = root.join("checkpoints");
    std::fs::create_dir_all(checkpoints.join("acestep-v15-turbo")).unwrap();
    std::fs::write(checkpoints.join("acestep-v15-turbo/w.safetensors"), "x").unwrap();
    let tools = Tools::new(server, library, checkpoints);

    call(
        &tools,
        "save_style",
        json!({"name": "new-wave", "tags": "synth pop, 120 bpm"}),
    )
    .await;
    call(
        &tools,
        "save_lyrics",
        json!({"name": "loop", "lyrics": "[verse]\nrun it again"}),
    )
    .await;
    let again = call_err(
        &tools,
        "save_style",
        json!({"name": "new-wave", "tags": "x"}),
    )
    .await;
    assert!(again.contains("already exists"), "{again}");
    let styles = call(&tools, "list_styles", json!({})).await;
    assert_eq!(
        styles["styles"][0],
        json!({"name": "new-wave", "tags": "synth pop, 120 bpm"})
    );

    let created = call(
        &tools,
        "create_song",
        json!({"style": "new-wave", "lyrics_name": "loop", "duration": 30, "takes": 2,
               "seed": 100, "steps": 8, "format": "wav"}),
    )
    .await;
    let jobs = created["jobs"].as_array().unwrap();
    assert_eq!(
        jobs.iter()
            .map(|j| j["seed"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [100, 101]
    );
    {
        let submitted = ace.submitted.lock().unwrap();
        assert_eq!(submitted[0]["prompt"], "synth pop, 120 bpm");
        assert_eq!(submitted[0]["lyrics"], "[verse]\nrun it again\n");
        assert_eq!(submitted[0]["inference_steps"], 8);
        assert_eq!(submitted[1]["seed"], 101);
    }

    let song = finished(&tools, &jobs[0]).await;
    finished(&tools, &jobs[1]).await;
    assert_eq!(song["name"], "loop-new-wave-100");
    assert!(file_exists(&song));
    assert_eq!(song["analysis"]["duration_secs"], 1.0);
    assert_eq!(song["analysis"]["clipping"], false);
    assert_eq!(song["info"]["key_scale"], "C major");

    let listed = call(&tools, "list_songs", json!({})).await;
    assert_eq!(listed["total"], 2);
    assert_eq!(listed["songs"][0]["model"], "fake-dit");
    let fetched = call(&tools, "get_song", json!({"name": "loop-new-wave-100"})).await;
    assert_eq!(fetched["request"]["seed"], 100);

    let varied = call(
        &tools,
        "variations",
        json!({"song": "loop-new-wave-100", "bpm": 90}),
    )
    .await;
    let new_song = finished(&tools, &varied["jobs"][0]).await;
    assert!(
        new_song["name"]
            .as_str()
            .unwrap()
            .starts_with("loop-new-wave-var-")
    );
    assert!(file_exists(&new_song));
    let resubmitted = ace.submitted.lock().unwrap().last().cloned().unwrap();
    assert_eq!(resubmitted["prompt"], "synth pop, 120 bpm");
    assert_eq!(resubmitted["bpm"], 90);
    assert_ne!(resubmitted["seed"], 100);

    let models = call(&tools, "list_models", json!({})).await;
    assert_eq!(models["installed"], json!(["acestep-v15-turbo"]));
    assert_eq!(models["backend"]["models"][0]["name"], "fake-dit");
    std::fs::remove_dir_all(&root).unwrap();
}

#[tokio::test]
async fn bad_requests_are_tool_errors_the_agent_can_read() {
    let (server, ace) = start_server().await;
    let root = scratch_dir("errors");
    let library = Library {
        songs: root.join("songs"),
        styles: root.join("styles"),
        lyrics: root.join("lyrics"),
    };
    let tools = Tools::new(server, library, root.join("none"));

    for (args, fragment) in [
        (json!({}), "exactly one of `style`"),
        (
            json!({"prompt": "x", "style": "y"}),
            "exactly one of `style`",
        ),
        (
            json!({"prompt": "x", "lyrics": "a", "lyrics_name": "b"}),
            "at most one of",
        ),
        (json!({"prompt": "x", "duration": 5}), "duration"),
        (json!({"prompt": "x", "takes": 9}), "takes"),
        (json!({"prompt": "x", "tempo": 120}), "invalid arguments"),
        (json!({"style": "missing"}), "missing.txt"),
    ] {
        let message = call_err(&tools, "create_song", args).await;
        assert!(
            message.contains(fragment),
            "{message} should mention {fragment}"
        );
    }
    assert!(ace.submitted.lock().unwrap().is_empty());

    let unknown = call_err(&tools, "get_job", json!({"job_id": "nope"})).await;
    assert!(unknown.contains("unknown job"), "{unknown}");
    assert!(matches!(
        tools.call("sing_to_me", json!({})).await,
        Err(ToolError::Unknown)
    ));
    std::fs::remove_dir_all(&root).unwrap();
}
