use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use tokio::process::{Child, Command};

use crate::{
    backend::Backend,
    server::{AppState, router},
};

const BACKEND_READY_TIMEOUT: Duration = Duration::from_secs(180);

pub struct ServeOptions {
    pub addr: SocketAddr,
    pub backend_url: String,
    pub backend_dir: Option<PathBuf>,
}

pub async fn serve(opts: ServeOptions) -> Result<()> {
    let backend = Arc::new(Backend::new(opts.backend_url.clone()));
    let mut child = match &opts.backend_dir {
        Some(dir) => Some(spawn_backend(dir, &opts.backend_url)?),
        None => None,
    };

    let result = run(&opts, backend, child.as_mut()).await;

    if let Some(child) = child.as_mut() {
        child.kill().await.context("stopping backend")?;
    }
    result
}

async fn run(opts: &ServeOptions, backend: Arc<Backend>, child: Option<&mut Child>) -> Result<()> {
    wait_ready(&backend, child, &opts.backend_url).await?;
    let listener = tokio::net::TcpListener::bind(opts.addr)
        .await
        .with_context(|| format!("binding {}", opts.addr))?;
    eprintln!("musicai listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(AppState { backend }))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("serving")
}

fn spawn_backend(dir: &Path, backend_url: &str) -> Result<Child> {
    let url = reqwest::Url::parse(backend_url).context("parsing --backend-url")?;
    let host = url.host_str().context("--backend-url has no host")?;
    let port = url
        .port_or_known_default()
        .context("--backend-url has no port")?;
    let dir = dir
        .canonicalize()
        .with_context(|| format!("backend dir {} not found", dir.display()))?;
    let program = dir.join(".venv/bin/acestep-api");
    if !program.exists() {
        bail!(
            "{} not found; run `uv sync` in {} first",
            program.display(),
            dir.display()
        );
    }
    eprintln!("starting ACE-Step backend on {host}:{port}");
    Command::new(program)
        .args(["--host", host, "--port", &port.to_string()])
        .current_dir(dir)
        .env("ACESTEP_LM_BACKEND", "mlx")
        .env("TOKENIZERS_PARALLELISM", "false")
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("spawning ACE-Step backend")
}

async fn wait_ready(backend: &Backend, mut child: Option<&mut Child>, url: &str) -> Result<()> {
    let deadline = Instant::now() + BACKEND_READY_TIMEOUT;
    loop {
        if backend.health().await {
            return Ok(());
        }
        match child.as_deref_mut() {
            Some(c) => {
                if let Some(status) = c.try_wait()? {
                    bail!("backend exited during startup: {status}");
                }
            }
            None => bail!("no backend reachable at {url}; start one or drop --no-backend"),
        }
        if Instant::now() >= deadline {
            bail!(
                "backend not ready after {}s",
                BACKEND_READY_TIMEOUT.as_secs()
            );
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
