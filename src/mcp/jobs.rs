use std::{
    collections::HashMap,
    hash::{BuildHasher, Hasher},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::{
    analysis::analyze_file,
    client,
    library::{Library, SongRecord},
    model::SongRequest,
};

const WAIT_STEP: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub enum JobState {
    Running { seed: i64 },
    Done(Box<SongRecord>),
    Failed { seed: i64, error: String },
}

#[derive(Debug, Serialize)]
pub struct Started {
    pub job_id: String,
    pub seed: i64,
}

/// Generation jobs started by this process. Each one waits for the REST server,
/// saves the audio and a sidecar into the library, and records the outcome.
pub struct Jobs {
    http: reqwest::Client,
    server: String,
    library: Library,
    state: Mutex<HashMap<String, JobState>>,
}

/// Seeds are recorded in each song so a good take can be recreated exactly.
pub fn random_seed() -> i64 {
    let bits = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    (bits % 1_000_000_000) as i64
}

impl Jobs {
    pub fn new(server: impl Into<String>, library: Library) -> Arc<Self> {
        Arc::new(Self {
            http: reqwest::Client::new(),
            server: server.into(),
            library,
            state: Mutex::new(HashMap::new()),
        })
    }

    /// Submits `takes` songs that differ only by seed: the given seed plus the
    /// take index, or random seeds. Files are named `<base>-<seed>`.
    pub async fn start(
        self: &Arc<Self>,
        template: SongRequest,
        takes: u32,
        base: &str,
    ) -> Result<Vec<Started>> {
        let mut started: Vec<Started> = Vec::new();
        for take in 0..takes {
            let seed = template
                .seed
                .map_or_else(random_seed, |s| s + i64::from(take));
            let request = SongRequest {
                seed: Some(seed),
                ..template.clone()
            };
            let status = match client::submit(&self.http, &self.server, &request).await {
                Ok(status) => status,
                Err(e) => {
                    let running: Vec<_> = started.iter().map(|s| s.job_id.as_str()).collect();
                    bail!(
                        "submitting take {} failed: {e:#} (already running: {running:?})",
                        take + 1
                    );
                }
            };
            self.set(&status.id, JobState::Running { seed });
            started.push(Started {
                job_id: status.id.clone(),
                seed,
            });
            tokio::spawn(Arc::clone(self).run(status.id, request, base.to_owned()));
        }
        Ok(started)
    }

    /// Current state of a job, waiting up to `wait` for it to finish.
    pub async fn get(&self, id: &str, wait: Duration) -> Result<JobState> {
        let deadline = Instant::now() + wait;
        loop {
            let state = self
                .state
                .lock()
                .expect("jobs lock")
                .get(id)
                .cloned()
                .ok_or_else(|| {
                    anyhow!(
                        "unknown job `{id}`; jobs only live as long as this server process, \
                         use list_songs for finished songs"
                    )
                })?;
            if !matches!(state, JobState::Running { .. }) || Instant::now() >= deadline {
                return Ok(state);
            }
            tokio::time::sleep(WAIT_STEP).await;
        }
    }

    fn set(&self, id: &str, state: JobState) {
        self.state
            .lock()
            .expect("jobs lock")
            .insert(id.to_owned(), state);
    }

    async fn run(self: Arc<Self>, id: String, request: SongRequest, base: String) {
        let seed = request.seed.unwrap_or_default();
        let state = match self.finish(&id, request, &base, seed).await {
            Ok(record) => JobState::Done(Box::new(record)),
            Err(e) => JobState::Failed {
                seed,
                error: format!("{e:#}"),
            },
        };
        self.set(&id, state);
    }

    async fn finish(
        &self,
        id: &str,
        request: SongRequest,
        base: &str,
        seed: i64,
    ) -> Result<SongRecord> {
        let status = client::wait(&self.http, &self.server, id).await?;
        let path = client::save_audio(
            &self.http,
            &self.server,
            id,
            &self.library.songs,
            &format!("{base}-{seed}"),
            request.format.extension(),
        )
        .await?;
        let for_analysis = path.clone();
        let (analysis, analysis_error) =
            match tokio::task::spawn_blocking(move || analyze_file(&for_analysis)).await? {
                Ok(analysis) => (Some(analysis), None),
                Err(e) => (None, Some(format!("{e:#}"))),
            };
        let text = |s: Option<&std::ffi::OsStr>| s.map(|s| s.to_string_lossy().into_owned());
        let record = SongRecord {
            name: text(path.file_stem()).ok_or_else(|| anyhow!("audio file has no name"))?,
            file: text(path.file_name()).ok_or_else(|| anyhow!("audio file has no name"))?,
            job_id: id.to_owned(),
            created_unix: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            request,
            info: status.info.unwrap_or_default(),
            analysis,
            analysis_error,
        };
        self.library.write_record(&record)?;
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_seeds_differ_and_fit_the_backend() {
        let seeds: Vec<_> = (0..8).map(|_| random_seed()).collect();
        assert!(seeds.iter().all(|s| (0..1_000_000_000).contains(s)));
        assert!(seeds.windows(2).any(|w| w[0] != w[1]));
    }
}
