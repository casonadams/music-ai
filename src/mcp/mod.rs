use std::{path::PathBuf, sync::Arc};

use anyhow::Result;

use crate::library::Library;

mod jobs;
pub mod protocol;
pub mod tools;

/// Serves the MCP tools over stdio until the client closes stdin.
pub async fn run(server: String, library: Library, checkpoints: PathBuf) -> Result<()> {
    let tools = Arc::new(tools::Tools::new(server, library, checkpoints));
    protocol::serve_stdio(tools).await
}
