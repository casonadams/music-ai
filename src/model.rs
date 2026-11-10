use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const MIN_DURATION_SECS: f32 = 10.0;
pub const MAX_DURATION_SECS: f32 = 600.0;
pub const MIN_BPM: u16 = 30;
pub const MAX_BPM: u16 = 300;
pub const MIN_STEPS: u32 = 1;
pub const MAX_STEPS: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Flac,
    #[default]
    Mp3,
    Wav,
}

impl AudioFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
        }
    }
}

impl FromStr for AudioFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "flac" => Ok(Self::Flac),
            "mp3" => Ok(Self::Mp3),
            "wav" => Ok(Self::Wav),
            other => Err(format!(
                "unknown format `{other}` (expected flac, mp3, wav)"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongRequest {
    pub prompt: String,
    #[serde(default)]
    pub lyrics: String,
    #[serde(default = "default_duration")]
    pub duration: f32,
    pub bpm: Option<u16>,
    pub key_scale: Option<String>,
    pub seed: Option<i64>,
    #[serde(default)]
    pub format: AudioFormat,
    /// DiT model name, e.g. `acestep-v15-xl-sft`; must be loaded by the backend.
    pub model: Option<String>,
    /// Diffusion steps: turbo models want about 8, base and sft about 32-64.
    pub steps: Option<u32>,
    /// Prompt guidance (CFG) strength. Affects base and sft models, not turbo.
    pub guidance_scale: Option<f32>,
    /// 5Hz LM checkpoint name, e.g. `acestep-5Hz-lm-4B`.
    pub lm_model: Option<String>,
    /// Extra backend fields (for example `guidance_scale`, `lm_temperature`).
    /// They never override the fields above.
    #[serde(default)]
    pub advanced: Map<String, Value>,
}

fn default_duration() -> f32 {
    60.0
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum InvalidRequest {
    #[error("prompt must not be empty")]
    EmptyPrompt,
    #[error("duration must be between {MIN_DURATION_SECS} and {MAX_DURATION_SECS} seconds")]
    Duration,
    #[error("bpm must be between {MIN_BPM} and {MAX_BPM}")]
    Bpm,
    #[error("steps must be between {MIN_STEPS} and {MAX_STEPS}")]
    Steps,
    #[error("guidance_scale must be a positive number")]
    Guidance,
}

impl SongRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            lyrics: String::new(),
            duration: default_duration(),
            bpm: None,
            key_scale: None,
            seed: None,
            format: AudioFormat::default(),
            model: None,
            steps: None,
            guidance_scale: None,
            lm_model: None,
            advanced: Map::new(),
        }
    }

    pub fn validate(&self) -> Result<(), InvalidRequest> {
        if self.prompt.trim().is_empty() {
            return Err(InvalidRequest::EmptyPrompt);
        }
        if !(MIN_DURATION_SECS..=MAX_DURATION_SECS).contains(&self.duration) {
            return Err(InvalidRequest::Duration);
        }
        if self.bpm.is_some_and(|b| !(MIN_BPM..=MAX_BPM).contains(&b)) {
            return Err(InvalidRequest::Bpm);
        }
        if self
            .steps
            .is_some_and(|s| !(MIN_STEPS..=MAX_STEPS).contains(&s))
        {
            return Err(InvalidRequest::Steps);
        }
        if self
            .guidance_scale
            .is_some_and(|g| !(g.is_finite() && g > 0.0))
        {
            return Err(InvalidRequest::Guidance);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SongState {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SongInfo {
    pub bpm: Option<f64>,
    pub key_scale: Option<String>,
    pub time_signature: Option<String>,
    pub seed: Option<String>,
    pub dit_model: Option<String>,
    pub lm_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongStatus {
    pub id: String,
    #[serde(rename = "status")]
    pub state: SongState,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<SongInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    /// Models load on first use, so this is false until a request needs them.
    #[serde(default)]
    pub is_loaded: bool,
}

/// The DiT and LM models the running backend is configured to serve.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelInventory {
    pub models: Vec<ModelEntry>,
    pub default_model: Option<String>,
    #[serde(default)]
    pub lm_models: Vec<ModelEntry>,
    pub loaded_lm_model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_enforces_bounds() {
        type Case = (fn(&mut SongRequest), Result<(), InvalidRequest>);
        let cases: [Case; 14] = [
            (|_| {}, Ok(())),
            (|r| r.prompt = "  ".into(), Err(InvalidRequest::EmptyPrompt)),
            (|r| r.duration = 9.9, Err(InvalidRequest::Duration)),
            (|r| r.duration = 10.0, Ok(())),
            (|r| r.duration = 600.0, Ok(())),
            (|r| r.duration = 600.1, Err(InvalidRequest::Duration)),
            (|r| r.bpm = Some(29), Err(InvalidRequest::Bpm)),
            (|r| r.bpm = Some(300), Ok(())),
            (|r| r.steps = Some(0), Err(InvalidRequest::Steps)),
            (|r| r.steps = Some(50), Ok(())),
            (|r| r.steps = Some(201), Err(InvalidRequest::Steps)),
            (
                |r| r.guidance_scale = Some(0.0),
                Err(InvalidRequest::Guidance),
            ),
            (
                |r| r.guidance_scale = Some(f32::NAN),
                Err(InvalidRequest::Guidance),
            ),
            (|r| r.guidance_scale = Some(4.0), Ok(())),
        ];
        for (mutate, expected) in cases {
            let mut r = SongRequest::new("pop punk");
            mutate(&mut r);
            assert_eq!(r.validate(), expected);
        }
    }

    #[test]
    fn request_json_applies_defaults() {
        let r: SongRequest = serde_json::from_str(r#"{"prompt":"lofi"}"#).unwrap();
        assert_eq!(r.duration, 60.0);
        assert_eq!(r.format, AudioFormat::Mp3);
        assert_eq!(r.lyrics, "");
    }

    #[test]
    fn format_parses_case_insensitively() {
        assert_eq!("MP3".parse(), Ok(AudioFormat::Mp3));
        assert!("ogg".parse::<AudioFormat>().is_err());
    }
}
