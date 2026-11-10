use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use musicai::{
    client::{generate, output_stem},
    download::{Selection, checkpoints_dir, download},
    library::Library,
    mcp,
    model::SongRequest,
    serve::{ServeOptions, serve},
};

use crate::cli::{DownloadArgs, GenerateArgs, McpArgs, ServeArgs};

pub async fn run_serve(args: ServeArgs) -> Result<()> {
    serve(ServeOptions {
        addr: args.addr,
        backend_url: args.backend_url,
        backend_dir: (!args.no_backend).then_some(args.backend_dir),
        models: args.models,
        lm_model: args.lm_model,
    })
    .await
}

pub async fn run_download(args: DownloadArgs) -> Result<()> {
    let selection = match (args.list, args.all) {
        (true, _) => Selection::List,
        (_, true) => Selection::All,
        _ => Selection::Models(args.models),
    };
    download(&args.backend_dir, &selection).await
}

pub async fn run_generate(args: GenerateArgs) -> Result<()> {
    let name = args
        .name
        .clone()
        .or_else(|| output_stem(args.lyrics_file.as_deref(), args.style_file.as_deref()));
    let request = song_request(&args)?;
    let path = generate(&args.server, &request, &args.out, name.as_deref()).await?;
    println!("{}", path.display());
    Ok(())
}

pub async fn run_mcp(args: McpArgs) -> Result<()> {
    let absolute =
        |p: PathBuf| std::path::absolute(&p).with_context(|| format!("resolving {}", p.display()));
    let library = Library {
        songs: absolute(args.out)?,
        styles: absolute(args.styles)?,
        lyrics: absolute(args.lyrics)?,
    };
    let checkpoints = checkpoints_dir(&absolute(args.backend_dir)?);
    mcp::run(args.server, library, checkpoints).await
}

fn song_request(args: &GenerateArgs) -> Result<SongRequest> {
    let prompt = match (&args.prompt, &args.style_file) {
        (Some(text), _) => text.clone(),
        (None, Some(path)) => read_text(path)?.trim().to_owned(),
        (None, None) => bail!("a style prompt or --style-file is required"),
    };
    let lyrics = match (&args.lyrics, &args.lyrics_file) {
        (Some(text), _) => text.clone(),
        (None, Some(path)) => read_text(path)?,
        (None, None) => String::new(),
    };
    let request = SongRequest {
        lyrics,
        duration: args.duration,
        bpm: args.bpm,
        key_scale: args.key.clone(),
        seed: args.seed,
        format: args.format,
        model: args.model.clone(),
        steps: args.steps,
        lm_model: args.lm_model.clone(),
        ..SongRequest::new(prompt)
    };
    request.validate()?;
    Ok(request)
}

fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}
