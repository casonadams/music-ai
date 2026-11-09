use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use musicai::{
    client::generate,
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
        /// Style tags, e.g. "late 90s pop punk, male vocals, 170 bpm"
        prompt: String,
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
        /// Output file name without extension (default: the lyrics file's name)
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
            let name = name.or_else(|| {
                lyrics_file
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
            });
            let lyrics = match (lyrics, lyrics_file) {
                (Some(text), _) => text,
                (None, Some(path)) => std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?,
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
