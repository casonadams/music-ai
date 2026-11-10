# Setup

## Requirements

- Apple Silicon Mac (M1 or newer). Minimum 16 GB RAM, aiming for about 12 GB of unified memory available to the model. This minimum is an estimate and has not been tested on a 16 GB Mac.
- Tested on an M5 Pro with 48 GB. With the default settings the backend picked its largest configuration (no offload, no quantization) and its process footprint was about 27 GB after one generation. On a smaller Mac, ACE-Step selects a smaller configuration from the detected memory.
- About 18 GB of disk for the downloaded models.
- [mise](https://mise.jdx.dev) (tasks, plus `mpv` for playback and `ffmpeg` for mp3), configured in `mise.toml`
- Rust toolchain and [uv](https://docs.astral.sh/uv/)

## Install

```sh
mise install     # mpv, ffmpeg
mise run setup   # scripts/setup.sh
```

`scripts/setup.sh` clones ACE-Step into `ACE-Step-1.5/` at the commit pinned in the script, then runs `uv sync` there. Bump the `REV` in the script to upgrade the backend.

`ACE-Step-1.5/`, `output/` and `target/` are gitignored.

## Run

```sh
mise run serve   # cargo run --release -- serve
```

`serve` launches `ACE-Step-1.5/.venv/bin/acestep-api` on `127.0.0.1:8001`, waits for its `/health`, then serves the REST API on `127.0.0.1:8000`. Ctrl-C stops both.

| Flag | Default | Meaning |
| --- | --- | --- |
| `--addr` (`MUSICAI_ADDR`) | `127.0.0.1:8000` | REST listen address |
| `--backend-url` | `http://127.0.0.1:8001` | ACE-Step API address (host and port are passed to the backend) |
| `--backend-dir` | `ACE-Step-1.5` | Checkout with a synced `.venv` |
| `--no-backend` | off | Use an ACE-Step API that is already running at `--backend-url` |
| `--model` | backend default (`acestep-v15-turbo`) | DiT model to load; repeat up to three times to serve several. The first is the default; requests pick one with `--model` |
| `--lm-model` | backend default | 5Hz LM, for example `acestep-5Hz-lm-4B` |

Run `serve` from the repo root so the default `--backend-dir` resolves, or pass a path. `mise run` always runs from the repo root.

## Models

Models are stored in `ACE-Step-1.5/checkpoints/` by default. Set `ACESTEP_CHECKPOINTS_DIR` to use another location, for example an external drive or a directory shared between installs.

```sh
musicai download --list                                  # what can be fetched
musicai download                                         # main bundle: turbo model, 1.7B LM, VAE, text encoder
musicai download acestep-v15-xl-sft acestep-5Hz-lm-4B    # higher quality
```

`musicai download` runs the backend's downloader in your terminal, so its per-file progress bars show. `--all` downloads every model. Models you have already downloaded are skipped.

| Model | Disk |
| --- | --- |
| `main` (turbo, 1.7B LM, VAE, text encoder) | about 9.5 GB |
| `acestep-v15-xl-sft` | about 20 GB |
| `acestep-5Hz-lm-4B` | about 8.4 GB |

## First run

If you skip `musicai download`, models download on the first generation request instead. Expect more than 10 GB of downloads and several minutes before the first song finishes. Later runs reuse the checkpoints directory.

Observed on an M5 Pro (48 GB) with the turbo model: about 25 to 40 seconds for a 30 second song, and about 80 to 110 seconds for a 150 second song.

## Formats and ffmpeg

`mp3` (default) export happens inside the ACE-Step backend and needs `ffmpeg` on the `PATH` of the `serve` process; `mise run serve` puts it on `PATH` for you. `flac` and `wav` need nothing extra, so use `-f flac` for lossless output or when running `serve` without mise.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| Song fails with "no audio file" | mp3 is the default and `ffmpeg` is not on the server's `PATH`. Start the server with `mise run serve`, or use `-f flac`. |
| `.venv/bin/acestep-api not found` | Run `mise run setup`, or pass the right `--backend-dir`. |
| `no backend reachable` | `--no-backend` was set but nothing listens at `--backend-url`. |
| `contacting musicai server` error from `generate` | `musicai serve` is not running, or `--server` points elsewhere. |
