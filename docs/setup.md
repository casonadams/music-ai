# Setup

## Requirements

- Apple Silicon Mac (tested: M1 Max, 64 GB)
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

Run `serve` from the repo root so the default `--backend-dir` resolves, or pass a path. `mise run` always runs from the repo root.

## First run

Models download on the first generation request, not at startup. Expect more than 10 GB of downloads and several minutes before the first song finishes. Later runs reuse `ACE-Step-1.5/checkpoints/`.

Observed on an M1 Max with the turbo model: about 25 to 40 seconds for a 30 second song, and about 80 seconds for a 150 second song.

## Formats and ffmpeg

`flac` (default) and `wav` need nothing extra. `mp3` export happens inside the ACE-Step backend and needs `ffmpeg` on the `PATH` of the `serve` process. `mise run serve` puts it on `PATH` for you.

## Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| Song fails with "no audio file" | You asked for mp3 and `ffmpeg` is not on the server's `PATH`. Use `--format flac` or fix `PATH`. |
| `.venv/bin/acestep-api not found` | Run `mise run setup`, or pass the right `--backend-dir`. |
| `no backend reachable` | `--no-backend` was set but nothing listens at `--backend-url`. |
| `contacting musicai server` error from `generate` | `musicai serve` is not running, or `--server` points elsewhere. |
