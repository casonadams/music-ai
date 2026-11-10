use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use musicai::{
    client::{generate, output_stem},
    model::{AudioFormat, SongRequest},
    serve::{ServeOptions, serve},
};

#[derive(Parser)]
#[command(version, about = "Local AI song generation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the REST server and its ACE-Step backend
    Serve {
        #[arg(long, env = "MUSICAI_ADDR", default_value = "127.0.0.1:8000")]
        addr: SocketAddr,
        #[arg(long, default_value = "http://127.0.0.1:8001")]
        backend_url: String,
        /// ACE-Step checkout (with a synced .venv) to launch as the backend
        #[arg(long, default_value = "ACE-Step-1.5")]
        backend_dir: PathBuf,
        /// Use a backend that is already running at --backend-url
        #[arg(long)]
        no_backend: bool,
    },
    /// Generate a song through a running server and save it to disk
    Generate {
        /// Style tags, e.g. "late 90s pop punk, male vocals, 170 bpm" (or use --style-file)
        #[arg(required_unless_present = "style_file", conflicts_with = "style_file")]
        prompt: Option<String>,
        /// File of style tags, e.g. styles/pop-punk.txt
        #[arg(long)]
        style_file: Option<PathBuf>,
        /// Lyrics with [verse]/[chorus] tags, or [instrumental]
        #[arg(long, conflicts_with = "lyrics_file")]
        lyrics: Option<String>,
        #[arg(long)]
        lyrics_file: Option<PathBuf>,
        /// Length in seconds (10-600)
        #[arg(short, long, default_value_t = 60.0)]
        duration: f32,
        #[arg(long)]
        bpm: Option<u16>,
        /// Musical key, e.g. "A minor"
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        seed: Option<i64>,
        /// mp3 (needs ffmpeg on the server's PATH), flac, or wav
        #[arg(short, long, default_value = "mp3", value_parser = str::parse::<AudioFormat>)]
        format: AudioFormat,
        #[arg(short, long, default_value = "output")]
        out: PathBuf,
        /// Output file name without extension (default: lyrics and style file names)
        #[arg(short, long)]
        name: Option<String>,
        #[arg(long, env = "MUSICAI_SERVER", default_value = "http://127.0.0.1:8000")]
        server: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve {
            addr,
            backend_url,
            backend_dir,
            no_backend,
        } => {
            serve(ServeOptions {
                addr,
                backend_url,
                backend_dir: (!no_backend).then_some(backend_dir),
            })
            .await
        }
        Command::Generate {
            prompt,
            style_file,
            lyrics,
            lyrics_file,
            duration,
            bpm,
            key,
            seed,
            format,
            out,
            name,
            server,
        } => {
            let name = name.or_else(|| output_stem(lyrics_file.as_deref(), style_file.as_deref()));
            let prompt = match (prompt, style_file) {
                (Some(text), _) => text,
                (None, Some(path)) => read_text(&path)?.trim().to_owned(),
                (None, None) => bail!("a style prompt or --style-file is required"),
            };
            let lyrics = match (lyrics, lyrics_file) {
                (Some(text), _) => text,
                (None, Some(path)) => read_text(&path)?,
                (None, None) => String::new(),
            };
            let request = SongRequest {
                prompt,
                lyrics,
                duration,
                bpm,
                key_scale: key,
                seed,
                format,
            };
            request.validate()?;
            let path = generate(&server, &request, &out, name.as_deref()).await?;
            println!("{}", path.display());
            Ok(())
        }
    }
}

fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}
