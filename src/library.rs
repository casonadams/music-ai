use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    analysis::Analysis,
    model::{SongInfo, SongRequest},
};

const MAX_NAME_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SongRecord {
    /// Audio file stem; also the sidecar's stem.
    pub name: String,
    pub file: String,
    pub job_id: String,
    pub created_unix: u64,
    pub request: SongRequest,
    pub info: SongInfo,
    pub analysis: Option<Analysis>,
    pub analysis_error: Option<String>,
}

/// Where songs are written and where reusable styles and lyrics live.
#[derive(Debug, Clone)]
pub struct Library {
    pub songs: PathBuf,
    pub styles: PathBuf,
    pub lyrics: PathBuf,
}

/// Names become file stems, so they are restricted to `[A-Za-z0-9_-]`.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        bail!("invalid name `{name}`: use 1-{MAX_NAME_LEN} letters, digits, `-` or `_`");
    }
    Ok(())
}

/// Stems of the `.txt` files in `dir`, sorted. A missing directory is empty.
pub fn list_text(dir: &Path) -> Result<Vec<String>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "txt")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            names.push(stem.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

pub fn read_text(dir: &Path, name: &str) -> Result<String> {
    validate_name(name)?;
    let path = dir.join(format!("{name}.txt"));
    fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
}

/// Writes `<dir>/<name>.txt`; refuses to replace an existing file unless `overwrite`.
pub fn write_text(dir: &Path, name: &str, text: &str, overwrite: bool) -> Result<PathBuf> {
    validate_name(name)?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{name}.txt"));
    let mut options = fs::OpenOptions::new();
    options.write(true);
    if overwrite {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(&path).map_err(|e| match e.kind() {
        ErrorKind::AlreadyExists => {
            anyhow::anyhow!(
                "{} already exists; pass overwrite to replace it",
                path.display()
            )
        }
        _ => anyhow::Error::new(e).context(format!("writing {}", path.display())),
    })?;
    file.write_all(text.as_bytes())?;
    Ok(path)
}

impl Library {
    pub fn write_record(&self, record: &SongRecord) -> Result<()> {
        let path = self.songs.join(format!("{}.json", record.name));
        let json = serde_json::to_vec_pretty(record)?;
        fs::write(&path, json).with_context(|| format!("writing {}", path.display()))
    }

    pub fn get_song(&self, name: &str) -> Result<SongRecord> {
        validate_name(name)?;
        read_record(&self.songs.join(format!("{name}.json")))
    }

    /// All songs, newest first. A sidecar that cannot be read is an error.
    pub fn list_songs(&self) -> Result<Vec<SongRecord>> {
        let entries = match fs::read_dir(&self.songs) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.songs.display())),
        };
        let mut songs = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                songs.push(read_record(&path)?);
            }
        }
        songs.sort_by(|a, b| {
            b.created_unix
                .cmp(&a.created_unix)
                .then(a.name.cmp(&b.name))
        });
        Ok(songs)
    }
}

fn read_record(path: &Path) -> Result<SongRecord> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("musicai-lib-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(name: &str, created_unix: u64) -> SongRecord {
        SongRecord {
            name: name.into(),
            file: format!("{name}.flac"),
            job_id: "job".into(),
            created_unix,
            request: SongRequest::new("lofi"),
            info: SongInfo::default(),
            analysis: None,
            analysis_error: None,
        }
    }

    #[test]
    fn names_cannot_escape_the_directory() {
        for bad in ["", "../x", "a/b", "a b", "a.txt", &"x".repeat(129)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
        for good in ["new-wave", "Take_2", "a1"] {
            assert!(validate_name(good).is_ok(), "{good}");
        }
    }

    #[test]
    fn text_files_round_trip_and_are_listed_sorted() {
        let dir = scratch_dir("text");
        write_text(&dir, "pop-punk", "tags", false).unwrap();
        write_text(&dir, "acoustic", "other", false).unwrap();
        fs::write(dir.join("notes.md"), "ignored").unwrap();
        assert_eq!(list_text(&dir).unwrap(), ["acoustic", "pop-punk"]);
        assert_eq!(read_text(&dir, "pop-punk").unwrap(), "tags");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn existing_text_is_only_replaced_when_asked() {
        let dir = scratch_dir("overwrite");
        write_text(&dir, "style", "one", false).unwrap();
        let err = write_text(&dir, "style", "two", false).unwrap_err();
        assert!(err.to_string().contains("already exists"));
        assert_eq!(read_text(&dir, "style").unwrap(), "one");
        write_text(&dir, "style", "two", true).unwrap();
        assert_eq!(read_text(&dir, "style").unwrap(), "two");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_directories_list_as_empty() {
        let dir = std::env::temp_dir().join("musicai-lib-does-not-exist");
        assert!(list_text(&dir).unwrap().is_empty());
        let library = Library {
            songs: dir.clone(),
            styles: dir.clone(),
            lyrics: dir,
        };
        assert!(library.list_songs().unwrap().is_empty());
    }

    #[test]
    fn songs_round_trip_and_list_newest_first() {
        let dir = scratch_dir("songs");
        let library = Library {
            songs: dir.clone(),
            styles: dir.clone(),
            lyrics: dir.clone(),
        };
        library.write_record(&record("old", 1)).unwrap();
        library.write_record(&record("new", 2)).unwrap();
        let names: Vec<_> = library
            .list_songs()
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["new", "old"]);
        assert_eq!(library.get_song("old").unwrap(), record("old", 1));
        assert!(library.get_song("../old").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
