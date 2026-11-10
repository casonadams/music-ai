use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use tokio::io::AsyncWriteExt;

use crate::model::{SongRequest, SongState, SongStatus};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub fn output_stem(lyrics_file: Option<&Path>, style_file: Option<&Path>) -> Option<String> {
    let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned());
    match (lyrics_file.and_then(stem), style_file.and_then(stem)) {
        (Some(lyrics), Some(style)) => Some(format!("{lyrics}-{style}")),
        (lyrics, style) => lyrics.or(style),
    }
}

pub async fn generate(
    server: &str,
    req: &SongRequest,
    out_dir: &Path,
    name: Option<&str>,
) -> Result<PathBuf> {
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
    let stem = match name {
        Some(name) => name.to_owned(),
        None => {
            let secs = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let id_prefix: String = created.id.chars().take(8).collect();
            format!("{secs}-{id_prefix}")
        }
    };
    write_unique(out_dir, &stem, req.format.extension(), &audio).await
}

pub async fn write_unique(dir: &Path, stem: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf> {
    if stem.is_empty() || stem.contains(['/', '\\']) {
        bail!("invalid file name `{stem}`: must be non-empty and contain no path separators");
    }
    for n in 1u32.. {
        let file = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        let path = dir.join(file);
        let opened = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await;
        match opened {
            Ok(mut f) => {
                f.write_all(bytes).await?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("writing {}", path.display())),
        }
    }
    unreachable!("u32 range exhausted")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("musicai-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn output_stem_combines_lyrics_and_style_names() {
        let lyrics = Path::new("lyrics/agentic-loop.txt");
        let style = Path::new("styles/new-wave.txt");
        assert_eq!(
            output_stem(Some(lyrics), Some(style)).as_deref(),
            Some("agentic-loop-new-wave")
        );
        assert_eq!(
            output_stem(Some(lyrics), None).as_deref(),
            Some("agentic-loop")
        );
        assert_eq!(output_stem(None, Some(style)).as_deref(), Some("new-wave"));
        assert_eq!(output_stem(None, None), None);
    }

    #[tokio::test]
    async fn write_unique_never_overwrites_an_existing_song() {
        let dir = scratch_dir("unique");
        let first = write_unique(&dir, "song", "mp3", b"one").await.unwrap();
        let second = write_unique(&dir, "song", "mp3", b"two").await.unwrap();
        let third = write_unique(&dir, "song", "mp3", b"three").await.unwrap();
        assert_eq!(first.file_name().unwrap(), "song.mp3");
        assert_eq!(second.file_name().unwrap(), "song-2.mp3");
        assert_eq!(third.file_name().unwrap(), "song-3.mp3");
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn write_unique_rejects_names_that_escape_the_directory() {
        let dir = scratch_dir("reject");
        for bad in ["", "../x", "a/b", "a\\b"] {
            assert!(write_unique(&dir, bad, "mp3", b"x").await.is_err(), "{bad}");
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
