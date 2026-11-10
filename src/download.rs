use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::backend::venv_tool;

pub const DEFAULT_MODEL: &str = "main";

#[derive(Debug, PartialEq, Eq)]
pub enum Selection {
    List,
    All,
    Models(Vec<String>),
}

/// One `acestep-download` argument list per run. The backend tool accepts a
/// single `--model` per run, so each requested model is its own run.
pub fn invocations(selection: &Selection) -> Vec<Vec<String>> {
    match selection {
        Selection::List => vec![vec!["--list".into()]],
        Selection::All => vec![vec!["--all".into()]],
        Selection::Models(models) if models.is_empty() => {
            vec![vec!["--model".into(), DEFAULT_MODEL.into()]]
        }
        Selection::Models(models) => models
            .iter()
            .map(|model| vec!["--model".into(), model.clone()])
            .collect(),
    }
}

/// Where the backend keeps models: `ACESTEP_CHECKPOINTS_DIR`, else
/// `<backend_dir>/checkpoints`.
pub fn checkpoints_dir(backend_dir: &Path) -> PathBuf {
    std::env::var_os("ACESTEP_CHECKPOINTS_DIR")
        .map_or_else(|| backend_dir.join("checkpoints"), PathBuf::from)
}

/// Sorted names of `acestep-*` model directories that contain weights.
/// A missing checkpoints directory means nothing is installed.
pub fn installed_models(checkpoints: &Path) -> Result<Vec<String>> {
    let entries = match std::fs::read_dir(checkpoints) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("reading {}", checkpoints.display()));
        }
    };
    let mut models = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("acestep-") && has_weights(&entry.path())? {
            models.push(name);
        }
    }
    models.sort();
    Ok(models)
}

fn has_weights(dir: &Path) -> Result<bool> {
    for file in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        if file?.path().extension().is_some_and(|e| e == "safetensors") {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Runs the backend's downloader with inherited stdio, so its own progress
/// bars reach the terminal. Honours `ACESTEP_CHECKPOINTS_DIR`.
pub async fn download(backend_dir: &Path, selection: &Selection) -> Result<()> {
    let (dir, program) = venv_tool(backend_dir, "acestep-download")?;
    for args in invocations(selection) {
        let status = Command::new(&program)
            .args(&args)
            .current_dir(&dir)
            .status()
            .await
            .context("running acestep-download")?;
        if !status.success() {
            bail!("acestep-download {} failed: {status}", args.join(" "));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models(names: &[&str]) -> Selection {
        Selection::Models(names.iter().map(|n| (*n).to_owned()).collect())
    }

    #[test]
    fn installed_models_need_weights_and_an_acestep_name() {
        let dir = std::env::temp_dir().join(format!("musicai-ckpt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (model, file) in [
            ("acestep-v15-turbo", "model.safetensors"),
            ("acestep-5Hz-lm-4B", "model-00001-of-00002.safetensors"),
            ("acestep-v15-xl-sft", "README.md"),
            ("vae", "diffusion_pytorch_model.safetensors"),
        ] {
            std::fs::create_dir_all(dir.join(model)).unwrap();
            std::fs::write(dir.join(model).join(file), "x").unwrap();
        }
        assert_eq!(
            installed_models(&dir).unwrap(),
            ["acestep-5Hz-lm-4B", "acestep-v15-turbo"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(installed_models(&dir).unwrap().is_empty());
    }

    #[test]
    fn no_models_downloads_main() {
        assert_eq!(invocations(&models(&[])), vec![vec!["--model", "main"]]);
    }

    #[test]
    fn each_model_is_its_own_run_in_order() {
        assert_eq!(
            invocations(&models(&["acestep-v15-xl-sft", "acestep-5Hz-lm-4B"])),
            vec![
                vec!["--model", "acestep-v15-xl-sft"],
                vec!["--model", "acestep-5Hz-lm-4B"],
            ]
        );
    }

    #[test]
    fn list_and_all_map_to_backend_flags() {
        assert_eq!(invocations(&Selection::List), vec![vec!["--list"]]);
        assert_eq!(invocations(&Selection::All), vec![vec!["--all"]]);
    }
}
