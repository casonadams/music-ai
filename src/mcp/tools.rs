use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

use super::{
    jobs::{JobState, Jobs, Started},
    protocol::{ToolError, ToolSet},
};
use crate::{
    client,
    download::installed_models,
    library::{self, Library, SongRecord, validate_name},
    model::{AudioFormat, SongRequest},
};

const MAX_TAKES: u32 = 4;
const MAX_WAIT_SECS: u64 = 30;
const DEFAULT_LIST_LIMIT: usize = 20;

const INSTRUCTIONS: &str = "\
Local song generation (ACE-Step). Typical flow: list_styles and list_lyrics to reuse material, \
save_lyrics to write new lyrics (section tags on their own line: [verse] [chorus] [bridge] \
[instrumental]), create_song (returns job ids; a song takes 1-10 minutes), get_job with \
wait_seconds until it succeeds, then give the user the file path.
You cannot hear audio. Judge a take only by its analysis (duration, loudness, true peak, \
clipping, near_silent) and ask the user which take they prefer. Every song records its seed \
and settings, so a liked take can be recreated or varied. Models are chosen when `musicai \
serve` starts: check list_models before asking for a specific one.";

pub struct Tools {
    jobs: Arc<Jobs>,
    library: Library,
    server: String,
    http: reqwest::Client,
    checkpoints: PathBuf,
}

impl Tools {
    pub fn new(server: String, library: Library, checkpoints: PathBuf) -> Self {
        Self {
            jobs: Jobs::new(server.clone(), library.clone()),
            library,
            server,
            http: reqwest::Client::new(),
            checkpoints,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveText {
    name: String,
    #[serde(alias = "tags", alias = "lyrics")]
    text: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameArg {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSong {
    style: Option<String>,
    prompt: Option<String>,
    lyrics: Option<String>,
    lyrics_name: Option<String>,
    name: Option<String>,
    duration: Option<f32>,
    bpm: Option<u16>,
    key: Option<String>,
    seed: Option<i64>,
    takes: Option<u32>,
    format: Option<AudioFormat>,
    model: Option<String>,
    steps: Option<u32>,
    lm_model: Option<String>,
    advanced: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variations {
    song: String,
    takes: Option<u32>,
    seed: Option<i64>,
    prompt: Option<String>,
    lyrics: Option<String>,
    duration: Option<f32>,
    bpm: Option<u16>,
    key: Option<String>,
    model: Option<String>,
    steps: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetJob {
    job_id: String,
    wait_seconds: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListSongs {
    limit: Option<usize>,
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T> {
    serde_json::from_value(args).map_err(|e| anyhow!("invalid arguments: {e}"))
}

fn check_takes(takes: Option<u32>) -> Result<u32> {
    let takes = takes.unwrap_or(1);
    if !(1..=MAX_TAKES).contains(&takes) {
        bail!("takes must be between 1 and {MAX_TAKES}");
    }
    Ok(takes)
}

/// Variations of `x-123` are named `x-var-<seed>`, never `x-123-var-<seed>`,
/// so names do not grow with each round.
fn variation_base(song: &str) -> String {
    let stem = match song.rsplit_once('-') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => song,
    };
    if stem.ends_with("-var") {
        stem.to_owned()
    } else {
        format!("{stem}-var")
    }
}

impl ToolSet for Tools {
    fn instructions(&self) -> &str {
        INSTRUCTIONS
    }

    fn definitions(&self) -> Vec<Value> {
        definitions()
    }

    async fn call(&self, name: &str, args: Value) -> Result<Value, ToolError> {
        match self.dispatch(name, args).await {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(ToolError::Unknown),
            Err(e) => Err(ToolError::Failed(format!("{e:#}"))),
        }
    }
}

impl Tools {
    async fn dispatch(&self, name: &str, args: Value) -> Result<Option<Value>> {
        let value = match name {
            "list_styles" => self.list_styles()?,
            "save_style" => self.save_text(&self.library.styles, "style", parse(args)?)?,
            "list_lyrics" => self.list_lyrics()?,
            "get_lyrics" => self.get_lyrics(parse(args)?)?,
            "save_lyrics" => self.save_text(&self.library.lyrics, "lyrics", parse(args)?)?,
            "create_song" => self.create_song(parse(args)?).await?,
            "variations" => self.variations(parse(args)?).await?,
            "get_job" => self.get_job(parse(args)?).await?,
            "list_songs" => self.list_songs(parse(args)?)?,
            "get_song" => self.get_song(parse(args)?)?,
            "list_models" => self.list_models().await?,
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    fn list_styles(&self) -> Result<Value> {
        let mut styles = Vec::new();
        for name in library::list_text(&self.library.styles)? {
            let tags = library::read_text(&self.library.styles, &name)?;
            styles.push(json!({"name": name, "tags": tags.trim()}));
        }
        Ok(json!({"styles": styles}))
    }

    fn list_lyrics(&self) -> Result<Value> {
        let mut lyrics = Vec::new();
        for name in library::list_text(&self.library.lyrics)? {
            let text = library::read_text(&self.library.lyrics, &name)?;
            let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
            lyrics.push(json!({"name": name, "lines": lines}));
        }
        Ok(json!({"lyrics": lyrics}))
    }

    fn get_lyrics(&self, args: NameArg) -> Result<Value> {
        let lyrics = library::read_text(&self.library.lyrics, &args.name)?;
        Ok(json!({"name": args.name, "lyrics": lyrics}))
    }

    fn save_text(&self, dir: &std::path::Path, kind: &str, args: SaveText) -> Result<Value> {
        let text = format!("{}\n", args.text.trim());
        let path = library::write_text(dir, &args.name, &text, args.overwrite)?;
        Ok(json!({"saved": kind, "name": args.name, "path": path}))
    }

    async fn create_song(&self, args: CreateSong) -> Result<Value> {
        let prompt = match (&args.prompt, &args.style) {
            (Some(prompt), None) => prompt.clone(),
            (None, Some(style)) => library::read_text(&self.library.styles, style)?
                .trim()
                .to_owned(),
            _ => bail!("give exactly one of `style` (a saved style name) or `prompt` (style tags)"),
        };
        let lyrics = match (&args.lyrics, &args.lyrics_name) {
            (Some(lyrics), None) => lyrics.clone(),
            (None, Some(name)) => library::read_text(&self.library.lyrics, name)?,
            (None, None) => String::new(),
            _ => bail!("give at most one of `lyrics` or `lyrics_name`"),
        };
        let base = match (&args.name, &args.lyrics_name, &args.style) {
            (Some(name), ..) => {
                validate_name(name)?;
                name.clone()
            }
            (None, Some(lyrics), Some(style)) => format!("{lyrics}-{style}"),
            (None, Some(only), None) | (None, None, Some(only)) => only.clone(),
            (None, None, None) => "song".to_owned(),
        };
        let defaults = SongRequest::new(prompt);
        let request = SongRequest {
            lyrics,
            duration: args.duration.unwrap_or(defaults.duration),
            bpm: args.bpm,
            key_scale: args.key,
            seed: args.seed,
            format: args.format.unwrap_or(AudioFormat::Flac),
            model: args.model,
            steps: args.steps,
            lm_model: args.lm_model,
            advanced: args.advanced.unwrap_or_default(),
            ..defaults
        };
        request.validate()?;
        self.start(request, check_takes(args.takes)?, &base).await
    }

    async fn variations(&self, args: Variations) -> Result<Value> {
        let original = self.library.get_song(&args.song)?.request;
        let request = SongRequest {
            prompt: args.prompt.unwrap_or(original.prompt),
            lyrics: args.lyrics.unwrap_or(original.lyrics),
            duration: args.duration.unwrap_or(original.duration),
            bpm: args.bpm.or(original.bpm),
            key_scale: args.key.or(original.key_scale),
            model: args.model.or(original.model),
            steps: args.steps.or(original.steps),
            seed: args.seed,
            ..original
        };
        request.validate()?;
        self.start(
            request,
            check_takes(args.takes)?,
            &variation_base(&args.song),
        )
        .await
    }

    async fn start(&self, request: SongRequest, takes: u32, base: &str) -> Result<Value> {
        let started: Vec<Started> = self.jobs.start(request, takes, base).await?;
        Ok(json!({"jobs": started}))
    }

    async fn get_job(&self, args: GetJob) -> Result<Value> {
        let wait = Duration::from_secs(args.wait_seconds.unwrap_or(0).min(MAX_WAIT_SECS));
        let id = &args.job_id;
        Ok(match self.jobs.get(id, wait).await? {
            JobState::Running { seed } => json!({"job_id": id, "status": "running", "seed": seed}),
            JobState::Done(record) => {
                json!({"job_id": id, "status": "succeeded", "song": self.song_json(&record)})
            }
            JobState::Failed { seed, error } => {
                json!({"job_id": id, "status": "failed", "seed": seed, "error": error})
            }
        })
    }

    fn list_songs(&self, args: ListSongs) -> Result<Value> {
        let songs = self.library.list_songs()?;
        let limit = args.limit.unwrap_or(DEFAULT_LIST_LIMIT);
        let summaries: Vec<Value> = songs.iter().take(limit).map(|s| self.summary(s)).collect();
        Ok(json!({"total": songs.len(), "songs": summaries}))
    }

    fn get_song(&self, args: NameArg) -> Result<Value> {
        Ok(self.song_json(&self.library.get_song(&args.name)?))
    }

    async fn list_models(&self) -> Result<Value> {
        let loaded = client::models(&self.http, &self.server).await?;
        Ok(json!({
            "installed": installed_models(&self.checkpoints)?,
            "backend": loaded,
        }))
    }

    fn song_json(&self, record: &SongRecord) -> Value {
        let mut value = serde_json::to_value(record).unwrap_or(Value::Null);
        value["path"] = json!(self.library.songs.join(&record.file));
        value
    }

    fn summary(&self, record: &SongRecord) -> Value {
        let analysis = record.analysis.as_ref();
        json!({
            "name": record.name,
            "path": self.library.songs.join(&record.file),
            "created_unix": record.created_unix,
            "prompt": record.request.prompt,
            "seed": record.request.seed,
            "model": record.info.dit_model,
            "bpm": record.info.bpm,
            "key": record.info.key_scale,
            "duration_secs": analysis.map(|a| a.duration_secs),
            "integrated_lufs": analysis.and_then(|a| a.integrated_lufs),
            "true_peak_dbtp": analysis.and_then(|a| a.true_peak_dbtp),
            "clipping": analysis.map(|a| a.clipping),
            "near_silent": analysis.map(|a| a.near_silent),
            "analysis_error": record.analysis_error,
        })
    }
}

fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

fn definitions() -> Vec<Value> {
    let name = |what: &str| json!({"type": "string", "description": what});
    let overwrite =
        json!({"type": "boolean", "description": "Replace an existing file (default false)"});
    let takes = json!({"type": "integer", "minimum": 1, "maximum": MAX_TAKES,
        "description": "How many takes (different seeds) to generate; default 1. Each take runs one after another."});
    vec![
        def(
            "list_styles",
            "List saved style presets: reusable comma-separated tags for genre, mood, instruments, vocals and tempo.",
            json!({}),
            &[],
        ),
        def(
            "save_style",
            "Save a style preset for reuse. Tags describe genre, mood, instruments, vocal type and tempo; do not name artists.",
            json!({"name": name("Letters, digits, - and _"), "tags": name("Comma-separated style tags"), "overwrite": overwrite}),
            &["name", "tags"],
        ),
        def(
            "list_lyrics",
            "List saved lyrics with their line counts.",
            json!({}),
            &[],
        ),
        def(
            "get_lyrics",
            "Read saved lyrics.",
            json!({"name": name("Lyrics name")}),
            &["name"],
        ),
        def(
            "save_lyrics",
            "Save lyrics. Put a section tag on its own line ([intro] [verse] [pre-chorus] [chorus] [bridge] [outro] [instrumental]) then short singable lines of similar length. Two verses and a chorus fit about 90-150 seconds.",
            json!({"name": name("Letters, digits, - and _"), "lyrics": name("The lyrics text"), "overwrite": overwrite}),
            &["name", "lyrics"],
        ),
        def(
            "create_song",
            "Start generating a song. Returns job ids immediately; poll get_job. Give a saved `style` or style tags in `prompt`; lyrics inline or by saved name (use [instrumental] for no vocals). Seeds are recorded so a good take can be recreated. Leave bpm and key unset unless the user asks: a wrong key can hurt the result.",
            json!({
                "style": name("Saved style name (see list_styles)"),
                "prompt": name("Style tags, used instead of `style`"),
                "lyrics": name("Lyrics text, used instead of `lyrics_name`"),
                "lyrics_name": name("Saved lyrics name (see list_lyrics)"),
                "name": name("File name base; default is derived from the lyrics and style names"),
                "duration": {"type": "number", "minimum": 10, "maximum": 600, "description": "Seconds; default 60"},
                "bpm": {"type": "integer", "minimum": 30, "maximum": 300},
                "key": name("Musical key such as \"A minor\""),
                "seed": {"type": "integer", "description": "Pin a seed; take N uses seed+N. Default: random"},
                "takes": takes,
                "format": {"type": "string", "enum": ["flac", "wav", "mp3"], "description": "Default flac; mp3 needs ffmpeg on the server's PATH"},
                "model": name("DiT model name; must be loaded (see list_models)"),
                "steps": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Diffusion steps: about 8 for turbo models, 32-64 for base and sft"},
                "lm_model": name("5Hz LM checkpoint name, e.g. acestep-5Hz-lm-4B"),
                "advanced": {"type": "object", "description": "Extra backend fields such as guidance_scale or lm_temperature. Never overrides the fields above."},
            }),
            &[],
        ),
        def(
            "variations",
            "Generate new takes of an existing song with a new seed (or the one you pass), optionally changing the prompt, lyrics, bpm, key, duration, steps or model.",
            json!({
                "song": name("Name of an existing song (see list_songs)"),
                "takes": takes,
                "seed": {"type": "integer"},
                "prompt": name("New style tags"),
                "lyrics": name("New lyrics text"),
                "duration": {"type": "number", "minimum": 10, "maximum": 600},
                "bpm": {"type": "integer", "minimum": 30, "maximum": 300},
                "key": name("Musical key such as \"A minor\""),
                "model": name("DiT model name"),
                "steps": {"type": "integer", "minimum": 1, "maximum": 200},
            }),
            &["song"],
        ),
        def(
            "get_job",
            "Status of a generation job started by create_song or variations. When it has succeeded the result includes the file path and analysis. Jobs only exist while this server process runs; wait_seconds long-polls up to 30.",
            json!({"job_id": name("Job id from create_song"), "wait_seconds": {"type": "integer", "minimum": 0, "maximum": MAX_WAIT_SECS}}),
            &["job_id"],
        ),
        def(
            "list_songs",
            "List finished songs, newest first, with seed, model and loudness measures (integrated LUFS, true peak, clipping, near_silent).",
            json!({"limit": {"type": "integer", "minimum": 1, "description": "Default 20"}}),
            &[],
        ),
        def(
            "get_song",
            "Full record of a song: the exact request, backend metadata (detected bpm and key) and analysis.",
            json!({"name": name("Song name from list_songs")}),
            &["name"],
        ),
        def(
            "list_models",
            "Models installed on disk (`installed`) and the DiT and LM models the running backend is configured to serve (`backend`; they load on first use, so is_loaded can be false). Only served models can be used for `model` and `lm_model`.",
            json!({}),
            &[],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variation_names_do_not_grow() {
        assert_eq!(variation_base("loop-wave-123"), "loop-wave-var");
        assert_eq!(variation_base("loop-wave-var-456"), "loop-wave-var");
        assert_eq!(variation_base("custom"), "custom-var");
        assert_eq!(variation_base("a-b"), "a-b-var");
    }

    #[test]
    fn every_tool_has_a_schema_and_a_unique_name() {
        let defs = definitions();
        let mut names: Vec<_> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();
        assert!(defs.iter().all(|d| d["inputSchema"]["type"] == "object"));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), defs.len());
    }

    #[test]
    fn takes_are_bounded() {
        assert_eq!(check_takes(None).unwrap(), 1);
        assert!(check_takes(Some(0)).is_err());
        assert!(check_takes(Some(MAX_TAKES + 1)).is_err());
    }
}
