mod cli;
mod commands;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve(args) => commands::run_serve(args).await,
        Command::Download(args) => commands::run_download(args).await,
        Command::Generate(args) => commands::run_generate(args).await,
        Command::Mcp(args) => commands::run_mcp(args).await,
    }
}
