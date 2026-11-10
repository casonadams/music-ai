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
    backend::{Backend, venv_tool},
    server::{AppState, router},
};

const BACKEND_READY_TIMEOUT: Duration = Duration::from_secs(180);

pub struct ServeOptions {
    pub addr: SocketAddr,
    pub backend_url: String,
    pub backend_dir: Option<PathBuf>,
    pub models: Vec<String>,
    pub lm_model: Option<String>,
}

pub async fn serve(opts: ServeOptions) -> Result<()> {
    let backend = Arc::new(Backend::new(opts.backend_url.clone()));
    let env = backend_env(&opts.models, opts.lm_model.as_deref())?;
    let mut child = match &opts.backend_dir {
        Some(dir) => Some(spawn_backend(dir, &opts.backend_url, &env)?),
        None if env.is_empty() => None,
        None => bail!("--model and --lm-model configure the launched backend; drop --no-backend"),
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

fn spawn_backend(dir: &Path, backend_url: &str, env: &[(&str, String)]) -> Result<Child> {
    let url = reqwest::Url::parse(backend_url).context("parsing --backend-url")?;
    let host = url.host_str().context("--backend-url has no host")?;
    let port = url
        .port_or_known_default()
        .context("--backend-url has no port")?;
    let (dir, program) = venv_tool(dir, "acestep-api")?;
    eprintln!("starting ACE-Step backend on {host}:{port}");
    Command::new(program)
        .args(["--host", host, "--port", &port.to_string()])
        .current_dir(dir)
        .env("ACESTEP_LM_BACKEND", "mlx")
        .env("TOKENIZERS_PARALLELISM", "false")
        .envs(env.iter().map(|(k, v)| (*k, v)))
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

const MAX_DIT_MODELS: usize = 3;

/// Backend environment selecting its DiT models (up to three, loaded side by
/// side; requests pick one by name) and its 5Hz LM.
fn backend_env(models: &[String], lm_model: Option<&str>) -> Result<Vec<(&'static str, String)>> {
    const SLOTS: [&str; MAX_DIT_MODELS] = [
        "ACESTEP_CONFIG_PATH",
        "ACESTEP_CONFIG_PATH2",
        "ACESTEP_CONFIG_PATH3",
    ];
    if models.len() > MAX_DIT_MODELS {
        bail!("at most {MAX_DIT_MODELS} --model values are supported");
    }
    let mut env: Vec<_> = SLOTS.into_iter().zip(models.iter().cloned()).collect();
    if let Some(lm) = lm_model {
        env.push(("ACESTEP_LM_MODEL_PATH", lm.to_owned()));
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn models_fill_backend_slots_in_order_and_lm_is_separate() {
        let env = backend_env(&names(&["xl-sft", "turbo"]), Some("lm-4B")).unwrap();
        assert_eq!(
            env,
            [
                ("ACESTEP_CONFIG_PATH", "xl-sft".to_owned()),
                ("ACESTEP_CONFIG_PATH2", "turbo".to_owned()),
                ("ACESTEP_LM_MODEL_PATH", "lm-4B".to_owned()),
            ]
        );
        assert!(backend_env(&[], None).unwrap().is_empty());
    }

    #[test]
    fn more_than_three_models_is_rejected() {
        assert!(backend_env(&names(&["a", "b", "c", "d"]), None).is_err());
    }
}
