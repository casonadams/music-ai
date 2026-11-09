use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::model::SongRequest;

const NO_AUDIO: &str = "backend reported success but produced no audio file \
(mp3 export needs ffmpeg on PATH; try --format flac)";

#[derive(Debug, PartialEq)]
pub enum TaskState {
    Running,
    Succeeded { file: String },
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
        let resp: Value = self
            .http
            .post(format!("{}/release_task", self.base))
            .json(&release_body(req))
            .send()
            .await
            .context("submitting task to backend")?
            .error_for_status()?
            .json()
            .await?;
        ensure_ok(&resp)?;
        resp.pointer("/data/task_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("backend response has no task_id"))
    }

    pub async fn task(&self, id: &str) -> Result<TaskState> {
        let resp: Value = self
            .http
            .post(format!("{}/query_result", self.base))
            .json(&json!({ "task_id_list": [id] }))
            .send()
            .await
            .context("querying backend task")?
            .error_for_status()?
            .json()
            .await?;
        parse_query_result(&resp)
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
    body
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
    fn release_body_pins_seed_and_metadata() {
        let mut req = request();
        req.bpm = Some(120);
        req.key_scale = Some("A minor".into());
        req.seed = Some(7);
        let body = release_body(&req);
        assert_eq!(body["bpm"], 120);
        assert_eq!(body["key_scale"], "A minor");
        assert_eq!(body["seed"], 7);
        assert_eq!(body["use_random_seed"], false);
    }

    #[test]
    fn parses_running_and_succeeded() {
        assert_eq!(
            parse_query_result(&entry(0, Value::Null)).unwrap(),
            TaskState::Running
        );
        let ok = entry(1, json!(r#"[{"file":"/v1/audio?path=x"}]"#));
        assert_eq!(
            parse_query_result(&ok).unwrap(),
            TaskState::Succeeded {
                file: "/v1/audio?path=x".into()
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
