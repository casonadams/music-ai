# music-ai

Local AI song generation on Apple Silicon. A small Rust CLI (`musicai`) that serves a REST API and generates songs on top of [ACE-Step 1.5](https://github.com/ACE-Step/ACE-Step-1.5) (MIT). Minimum: an M1 or newer Mac with 16 GB RAM (see [Setup](docs/setup.md#requirements)). Tested on an M5 Pro with 48 GB.

## Quick start

Requires [mise](https://mise.jdx.dev), [uv](https://docs.astral.sh/uv/) and a Rust toolchain.

```sh
mise install        # mpv, ffmpeg
mise run setup      # clone pinned ACE-Step backend, install its Python env

# terminal 1: start the server (launches the ACE-Step backend too)
mise run serve

# terminal 2: pick a style and lyrics from the repo, or write your own files
mise run generate -- --style-file styles/acoustic-rock.txt --lyrics-file lyrics/agentic-ship-easy.txt -d 120

# play it
mpv output/
```

Other tasks: `mise tasks` lists them; `mise run test` runs clippy and the tests. Without mise, the same commands are `scripts/setup.sh`, `cargo run --release -- serve` and `cargo run --release -- generate ...`.

## Commands

| Command | Purpose |
| --- | --- |
| `musicai serve` | Start the REST API on `127.0.0.1:8000` and the ACE-Step backend on `:8001` |
| `musicai download` | Download models into `ACE-Step-1.5/checkpoints/` with progress (`--list` shows the options, see [Setup](docs/setup.md#models)) |
| `musicai generate` | Submit a song, wait, save the audio to `output/`, print the path |
| `musicai mcp` | Serve MCP tools over stdio so an AI agent can write lyrics and generate songs; needs `musicai serve` (see [MCP](docs/mcp.md)) |

`musicai generate` takes a style and lyrics, each from a file (preferred) or inline:

| Input | From a file | Inline |
| --- | --- | --- |
| Style tags | `--style-file styles/new-wave.txt` | positional `"<style tags>"` |
| Lyrics | `--lyrics-file lyrics/agentic-loop.txt` | `--lyrics "<text>"` |

Other flags: `-d/--duration` (10-600 s, default 60), `--bpm`, `--key`, `--seed`, `-f/--format` (`mp3` default, `flac`, `wav`), `--model`, `--steps`, `--lm-model` (see [Setup](docs/setup.md#models)), `-n/--name`, `-o/--out`, `--server`. Run `musicai <command> --help` for details.

Styles and lyrics are separate so you can swap either one: `styles/` holds reusable style tags (genre, instruments, vocals, tempo) and `lyrics/` holds songs' words with `[verse]` and `[chorus]` section tags (see [Prompting](docs/prompting.md)). Output files are named `<lyrics>-<style>` from the file names (`agentic-loop` + `new-wave` gives `output/agentic-loop-new-wave.mp3`), or from `--name`, or `<timestamp>-<id>` when inline. An existing file is never overwritten; a repeat run gets `-2`, `-3`, and so on.

## REST API

| Method | Path | Result |
| --- | --- | --- |
| `POST` | `/v1/songs` | `202 {"id","status":"running"}`; body: `prompt`, `lyrics`, `duration`, `bpm`, `key_scale`, `seed`, `format`, `model`, `steps`, `lm_model`, `advanced` |
| `GET` | `/v1/songs/{id}` | `{"id","status":"running\|succeeded\|failed","error","info"}`; `info` (on success) holds the backend's detected bpm and key, seed and models |
| `GET` | `/v1/songs/{id}/audio` | Audio bytes; `409` until the song has succeeded |
| `GET` | `/v1/models` | DiT and LM models the backend is configured to serve |
| `GET` | `/health` | `ok` |

```sh
curl -s localhost:8000/v1/songs -H 'Content-Type: application/json' \
  -d '{"prompt":"lofi hip hop, mellow piano","lyrics":"[instrumental]","duration":30}'
```

## Docs

- [Setup](docs/setup.md): install, first run, mp3 and ffmpeg, troubleshooting
- [MCP server](docs/mcp.md): let an AI agent write lyrics and generate songs
- [Prompting](docs/prompting.md): style tags, lyrics structure
- [Examples](docs/examples.md): full prompts that have been run

## License

MIT, see [LICENSE](LICENSE). The ACE-Step backend is a separate project under its own MIT license.
