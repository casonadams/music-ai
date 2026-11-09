use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};

use crate::model::{SongRequest, SongState, SongStatus};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub async fn generate(server: &str, req: &SongRequest, out_dir: &Path) -> Result<PathBuf> {
    let server = server.trim_end_matches('/');
    let http = reqwest::Client::new();

    let created: SongStatus = ok_json(http.post(format!("{server}/v1/songs")).json(req)).await?;
    eprintln!("generating {}", created.id);

    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        let status: SongStatus =
            ok_json(http.get(format!("{server}/v1/songs/{}", created.id))).await?;
        match status.state {
            SongState::Running => continue,
            SongState::Succeeded => break,
            SongState::Failed => bail!(status.error.unwrap_or_else(|| "generation failed".into())),
        }
    }

    let audio = http
        .get(format!("{server}/v1/songs/{}/audio", created.id))
        .send()
        .await
        .context("downloading audio")?;
    let audio = ensure_success(audio).await?.bytes().await?;

    tokio::fs::create_dir_all(out_dir).await?;
    let secs = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let id_prefix: String = created.id.chars().take(8).collect();
    let path = out_dir.join(format!("{secs}-{id_prefix}.{}", req.format.extension()));
    tokio::fs::write(&path, &audio).await?;
    Ok(path)
}

async fn ok_json<T: serde::de::DeserializeOwned>(req: reqwest::RequestBuilder) -> Result<T> {
    let resp = req
        .send()
        .await
        .context("contacting musicai server (is `musicai serve` running?)")?;
    Ok(ensure_success(resp).await?.json().await?)
}

async fn ensure_success(resp: reqwest::Response) -> Result<reqwest::Response> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    bail!("server returned {status}: {body}")
}
