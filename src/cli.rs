use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser, Subcommand};
use musicai::model::AudioFormat;

#[derive(Parser)]
#[command(version, about = "Local AI song generation")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the REST server and its ACE-Step backend
    Serve(ServeArgs),
    /// Download ACE-Step models into the backend's checkpoints directory
    ///
    /// Shows the downloader's progress. Set ACESTEP_CHECKPOINTS_DIR to store
    /// models somewhere other than <backend-dir>/checkpoints.
    Download(DownloadArgs),
    /// Generate a song through a running server and save it to disk
    Generate(GenerateArgs),
    /// Serve MCP tools over stdio so an agent can build songs (needs `musicai serve`)
    Mcp(McpArgs),
}

#[derive(Args)]
pub struct ServeArgs {
    #[arg(long, env = "MUSICAI_ADDR", default_value = "127.0.0.1:8000")]
    pub addr: SocketAddr,
    #[arg(long, default_value = "http://127.0.0.1:8001")]
    pub backend_url: String,
    /// ACE-Step checkout (with a synced .venv) to launch as the backend
    #[arg(long, default_value = "ACE-Step-1.5")]
    pub backend_dir: PathBuf,
    /// Use a backend that is already running at --backend-url
    #[arg(long)]
    pub no_backend: bool,
    /// DiT model(s) to load, e.g. acestep-v15-xl-sft; repeat for up to three
    #[arg(long = "model")]
    pub models: Vec<String>,
    /// 5Hz LM to load, e.g. acestep-5Hz-lm-4B
    #[arg(long)]
    pub lm_model: Option<String>,
    /// Audio VAE to use: `official` (default) or `scragvae`, or a directory path
    #[arg(long)]
    pub vae: Option<String>,
}

#[derive(Args)]
pub struct DownloadArgs {
    /// Models to fetch, e.g. acestep-v15-xl-sft acestep-5Hz-lm-4B (default: main)
    #[arg(conflicts_with_all = ["list", "all"])]
    pub models: Vec<String>,
    /// List the models that can be downloaded
    #[arg(long, conflicts_with = "all")]
    pub list: bool,
    /// Download every model
    #[arg(long)]
    pub all: bool,
    /// ACE-Step checkout (with a synced .venv)
    #[arg(long, default_value = "ACE-Step-1.5")]
    pub backend_dir: PathBuf,
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Style tags, e.g. "late 90s pop punk, male vocals, 170 bpm" (or use --style-file)
    #[arg(required_unless_present = "style_file", conflicts_with = "style_file")]
    pub prompt: Option<String>,
    /// File of style tags, e.g. styles/pop-punk.txt
    #[arg(long)]
    pub style_file: Option<PathBuf>,
    /// Lyrics with [verse]/[chorus] tags, or [instrumental]
    #[arg(long, conflicts_with = "lyrics_file")]
    pub lyrics: Option<String>,
    #[arg(long)]
    pub lyrics_file: Option<PathBuf>,
    /// Length in seconds (10-600)
    #[arg(short, long, default_value_t = 60.0)]
    pub duration: f32,
    #[arg(long)]
    pub bpm: Option<u16>,
    /// Musical key, e.g. "A minor"
    #[arg(long)]
    pub key: Option<String>,
    #[arg(long)]
    pub seed: Option<i64>,
    /// mp3 (needs ffmpeg on the server's PATH), flac, or wav
    #[arg(short, long, default_value = "mp3", value_parser = str::parse::<AudioFormat>)]
    pub format: AudioFormat,
    #[arg(short, long, default_value = "output")]
    pub out: PathBuf,
    /// Output file name without extension (default: lyrics and style file names)
    #[arg(short, long)]
    pub name: Option<String>,
    #[arg(long, env = "MUSICAI_SERVER", default_value = "http://127.0.0.1:8000")]
    pub server: String,
    /// DiT model to use (must be loaded by the backend, see `serve --model`)
    #[arg(long)]
    pub model: Option<String>,
    /// Diffusion steps (about 8 for turbo, 32-64 for base and sft)
    #[arg(long)]
    pub steps: Option<u32>,
    /// Prompt guidance for base and sft models (turbo ignores it); 4 sounded cleaner than 7
    #[arg(long)]
    pub guidance_scale: Option<f32>,
    /// 5Hz LM checkpoint, e.g. acestep-5Hz-lm-4B
    #[arg(long)]
    pub lm_model: Option<String>,
}

#[derive(Args)]
pub struct McpArgs {
    #[arg(long, env = "MUSICAI_SERVER", default_value = "http://127.0.0.1:8000")]
    pub server: String,
    /// Where finished songs and their metadata are saved
    #[arg(long, default_value = "output")]
    pub out: PathBuf,
    /// Directory of reusable style tag files
    #[arg(long, default_value = "styles")]
    pub styles: PathBuf,
    /// Directory of lyrics files
    #[arg(long, default_value = "lyrics")]
    pub lyrics: PathBuf,
    /// ACE-Step checkout, used to see which models are installed
    #[arg(long, default_value = "ACE-Step-1.5")]
    pub backend_dir: PathBuf,
}
