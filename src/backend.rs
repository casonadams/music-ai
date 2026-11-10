use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::model::{ModelInventory, SongInfo, SongRequest};

const NO_AUDIO: &str = "backend reported success but produced no audio file \
(mp3 export needs ffmpeg on PATH; try --format flac)";

#[derive(Debug, PartialEq)]
pub enum TaskState {
    Running,
    Succeeded { file: String, info: SongInfo },
    Failed(String),
}

pub struct Backend {
    http: reqwest::Client,
    base: String,
}

impl Backend {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn health(&self) -> bool {
        self.http
            .get(format!("{}/health", self.base))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }

    pub async fn submit(&self, req: &SongRequest) -> Result<String> {
        let request = self
            .http
            .post(format!("{}/release_task", self.base))
            .json(&release_body(req));
        let resp = self.json_of(request, "submitting task to backend").await?;
        ensure_ok(&resp)?;
        resp.pointer("/data/task_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("backend response has no task_id"))
    }

    pub async fn task(&self, id: &str) -> Result<TaskState> {
        let request = self
            .http
            .post(format!("{}/query_result", self.base))
            .json(&json!({ "task_id_list": [id] }));
        parse_query_result(&self.json_of(request, "querying backend task").await?)
    }

    pub async fn audio(&self, file: &str) -> Result<reqwest::Response> {
        Ok(self
            .http
            .get(format!("{}{}", self.base, file))
            .send()
            .await
            .context("fetching audio from backend")?
            .error_for_status()?)
    }

    pub async fn models(&self) -> Result<ModelInventory> {
        let request = self.http.get(format!("{}/v1/model_inventory", self.base));
        parse_models(&self.json_of(request, "listing backend models").await?)
    }

    async fn json_of(&self, request: reqwest::RequestBuilder, what: &'static str) -> Result<Value> {
        Ok(request
            .send()
            .await
            .context(what)?
            .error_for_status()?
            .json()
            .await?)
    }
}

/// Resolves `tool` in the synced `.venv` of the ACE-Step checkout `dir`.
/// Returns the canonical checkout dir and the executable path.
pub fn venv_tool(dir: &Path, tool: &str) -> Result<(PathBuf, PathBuf)> {
    let dir = dir
        .canonicalize()
        .with_context(|| format!("backend dir {} not found", dir.display()))?;
    let program = dir.join(".venv/bin").join(tool);
    if !program.exists() {
        bail!(
            "{} not found; run `mise run setup` (or `uv sync` in {}) first",
            program.display(),
            dir.display()
        );
    }
    Ok((dir, program))
}

pub fn release_body(req: &SongRequest) -> Value {
    let mut body = json!({
        "prompt": req.prompt,
        "lyrics": req.lyrics,
        "audio_duration": req.duration,
        "audio_format": req.format.extension(),
        "thinking": true,
        "batch_size": 1,
    });
    let fields = body.as_object_mut().expect("object literal");
    if let Some(bpm) = req.bpm {
        fields.insert("bpm".into(), json!(bpm));
    }
    if let Some(key) = &req.key_scale {
        fields.insert("key_scale".into(), json!(key));
    }
    if let Some(seed) = req.seed {
        fields.insert("use_random_seed".into(), json!(false));
        fields.insert("seed".into(), json!(seed));
    }
    if let Some(model) = &req.model {
        fields.insert("model".into(), json!(model));
    }
    if let Some(steps) = req.steps {
        fields.insert("inference_steps".into(), json!(steps));
    }
    if let Some(lm) = &req.lm_model {
        fields.insert("lm_model_path".into(), json!(lm));
    }
    for (key, value) in &req.advanced {
        fields.entry(key.clone()).or_insert_with(|| value.clone());
    }
    body
}

fn parse_info(item: &Value) -> SongInfo {
    let text = |v: &Value| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let metas = &item["metas"];
    SongInfo {
        bpm: metas["bpm"]
            .as_f64()
            .or_else(|| metas["bpm"].as_str().and_then(|s| s.parse().ok())),
        key_scale: text(&metas["keyscale"]),
        time_signature: text(&metas["timesignature"]),
        seed: text(&item["seed_value"]),
        dit_model: text(&item["dit_model"]),
        lm_model: text(&item["lm_model"]),
    }
}

pub fn parse_models(resp: &Value) -> Result<ModelInventory> {
    ensure_ok(resp)?;
    serde_json::from_value(resp["data"].clone()).context("parsing backend model inventory")
}

fn ensure_ok(resp: &Value) -> Result<()> {
    if resp["code"].as_i64() == Some(200) {
        return Ok(());
    }
    bail!(
        "backend error: {}",
        resp["error"].as_str().unwrap_or("unknown error")
    )
}

pub fn parse_query_result(resp: &Value) -> Result<TaskState> {
    let task = resp
        .pointer("/data/0")
        .ok_or_else(|| anyhow!("backend returned no task entry"))?;
    let result = task["result"].as_str().unwrap_or_default();
    match task["status"].as_i64() {
        Some(0) => Ok(TaskState::Running),
        Some(1) => {
            let items: Value = serde_json::from_str(result).context("parsing task result")?;
            let file = items
                .pointer("/0/file")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if file.is_empty() {
                Ok(TaskState::Failed(NO_AUDIO.into()))
            } else {
                Ok(TaskState::Succeeded {
                    file: file.to_owned(),
                    info: parse_info(&items[0]),
                })
            }
        }
        Some(2) => Ok(TaskState::Failed(if result.is_empty() {
            "backend generation failed".into()
        } else {
            format!("backend generation failed: {result}")
        })),
        other => bail!("unexpected backend task status: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> SongRequest {
        SongRequest {
            duration: 30.0,
            ..SongRequest::new("lofi")
        }
    }

    fn entry(status: i64, result: Value) -> Value {
        json!({"data": [{"task_id": "t", "status": status, "result": result}], "code": 200})
    }

    #[test]
    fn release_body_maps_required_fields() {
        let body = release_body(&request());
        assert_eq!(body["prompt"], "lofi");
        assert_eq!(body["audio_duration"], 30.0);
        assert_eq!(body["audio_format"], "mp3");
        assert!(body.get("bpm").is_none());
        assert!(body.get("seed").is_none());
    }

    #[test]
    fn release_body_maps_optional_fields() {
        let mut req = request();
        req.bpm = Some(120);
        req.key_scale = Some("A minor".into());
        req.seed = Some(7);
        req.model = Some("acestep-v15-xl-sft".into());
        req.steps = Some(50);
        req.lm_model = Some("acestep-5Hz-lm-4B".into());
        let body = release_body(&req);
        assert_eq!(body["bpm"], 120);
        assert_eq!(body["key_scale"], "A minor");
        assert_eq!(body["seed"], 7);
        assert_eq!(body["use_random_seed"], false);
        assert_eq!(body["model"], "acestep-v15-xl-sft");
        assert_eq!(body["inference_steps"], 50);
        assert_eq!(body["lm_model_path"], "acestep-5Hz-lm-4B");
    }

    #[test]
    fn advanced_fields_are_added_but_never_override_core_fields() {
        let mut req = request();
        req.advanced.insert("guidance_scale".into(), json!(9.0));
        req.advanced.insert("prompt".into(), json!("hijacked"));
        req.advanced.insert("batch_size".into(), json!(8));
        let body = release_body(&req);
        assert_eq!(body["guidance_scale"], 9.0);
        assert_eq!(body["prompt"], "lofi");
        assert_eq!(body["batch_size"], 1);
    }

    #[test]
    fn parses_the_model_inventory() {
        let resp = json!({"code": 200, "data": {
            "models": [{"name": "a", "is_default": true, "is_loaded": false, "supported_task_types": []},
                       {"name": "b", "is_default": false, "is_loaded": true}],
            "default_model": "a",
            "lm_models": [{"name": "lm", "is_loaded": false}],
            "loaded_lm_model": null,
            "llm_initialized": false}});
        let inventory = parse_models(&resp).unwrap();
        assert_eq!(inventory.default_model.as_deref(), Some("a"));
        assert_eq!(inventory.models.len(), 2);
        assert!(inventory.models[0].is_default && inventory.models[1].is_loaded);
        assert_eq!(inventory.lm_models[0].name, "lm");
        assert!(parse_models(&json!({"code": 500, "error": "no"})).is_err());
    }

    #[test]
    fn parses_running_and_succeeded() {
        assert_eq!(
            parse_query_result(&entry(0, Value::Null)).unwrap(),
            TaskState::Running
        );
        let ok = entry(
            1,
            json!(
                r#"[{"file":"/v1/audio?path=x","seed_value":"42","dit_model":"acestep-v15-turbo",
                "lm_model":"acestep-5Hz-lm-1.7B",
                "metas":{"bpm":120,"keyscale":"A minor","timesignature":"4"}}]"#
            ),
        );
        assert_eq!(
            parse_query_result(&ok).unwrap(),
            TaskState::Succeeded {
                file: "/v1/audio?path=x".into(),
                info: SongInfo {
                    bpm: Some(120.0),
                    key_scale: Some("A minor".into()),
                    time_signature: Some("4".into()),
                    seed: Some("42".into()),
                    dit_model: Some("acestep-v15-turbo".into()),
                    lm_model: Some("acestep-5Hz-lm-1.7B".into()),
                }
            }
        );
    }

    #[test]
    fn success_tolerates_missing_metadata() {
        let ok = entry(1, json!(r#"[{"file":"/v1/audio?path=x"}]"#));
        assert_eq!(
            parse_query_result(&ok).unwrap(),
            TaskState::Succeeded {
                file: "/v1/audio?path=x".into(),
                info: SongInfo::default()
            }
        );
    }

    #[test]
    fn success_without_file_is_a_failure() {
        let empty = entry(1, json!(r#"[{"file":""}]"#));
        assert_eq!(
            parse_query_result(&empty).unwrap(),
            TaskState::Failed(NO_AUDIO.into())
        );
    }

    #[test]
    fn failed_status_carries_backend_detail() {
        let failed = entry(2, json!("out of memory"));
        assert_eq!(
            parse_query_result(&failed).unwrap(),
            TaskState::Failed("backend generation failed: out of memory".into())
        );
    }

    #[test]
    fn unknown_status_is_an_error() {
        assert!(parse_query_result(&entry(9, Value::Null)).is_err());
        assert!(parse_query_result(&json!({"data": []})).is_err());
    }
}
