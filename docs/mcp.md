# MCP server

`musicai mcp` lets an AI agent write lyrics, generate songs and review the results. It speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio (protocol 2025-06-18, tools only) and talks to a running `musicai serve`.

## Setup

Start the server first (`mise run serve`, or `musicai serve --model acestep-v15-xl-sft --lm-model acestep-5Hz-lm-4B` for the best quality, see [Setup](setup.md#models)). Then register the MCP server with your client, using absolute paths so it does not depend on the client's working directory:

```json
{
  "mcpServers": {
    "musicai": {
      "command": "/path/to/music-ai/target/release/musicai",
      "args": [
        "mcp",
        "--out", "/path/to/music-ai/output",
        "--styles", "/path/to/music-ai/styles",
        "--lyrics", "/path/to/music-ai/lyrics",
        "--backend-dir", "/path/to/music-ai/ACE-Step-1.5"
      ]
    }
  }
}
```

With Claude Code: `claude mcp add musicai -- /path/to/musicai mcp --out /path/to/output ...`.

| Flag | Default | Meaning |
| --- | --- | --- |
| `--server` (`MUSICAI_SERVER`) | `http://127.0.0.1:8000` | The `musicai serve` REST address |
| `--out` | `output` | Where songs and their metadata are written |
| `--styles` | `styles` | Style tag files (`<name>.txt`) |
| `--lyrics` | `lyrics` | Lyrics files (`<name>.txt`) |
| `--backend-dir` | `ACE-Step-1.5` | Used to see which models are installed (honours `ACESTEP_CHECKPOINTS_DIR`) |

## Tools

| Tool | Purpose |
| --- | --- |
| `list_styles`, `save_style` | Reusable style tag presets |
| `list_lyrics`, `get_lyrics`, `save_lyrics` | Lyrics with `[verse]`/`[chorus]` tags |
| `create_song` | Start 1-4 takes from a saved or inline style and lyrics; returns job ids at once |
| `variations` | New takes of an existing song with a new seed, optionally changing the prompt, lyrics, bpm, key, duration, steps or model |
| `get_job` | Job status; `wait_seconds` (up to 30) long-polls. A finished job returns the file path and analysis |
| `list_songs`, `get_song` | Finished songs, newest first, with the exact request, backend metadata and analysis |
| `list_models` | Models installed on disk, and the models the running backend is configured to serve (they load on first use) |

`create_song` accepts `model`, `steps`, `lm_model`, `bpm`, `key`, `seed`, `format` (default `flac`) and an `advanced` object forwarded to the backend (for example `guidance_scale`, `lm_temperature`; it never overrides the named fields). Leave `bpm` and `key` unset unless you need them.

## Songs and metadata

Each take is saved as `<name>-<seed>.<format>` with a `<name>-<seed>.json` sidecar holding the request, the backend's metadata (detected bpm and key, models used) and an analysis. Analysis is computed in Rust (EBU R128 via `symphonia` and `ebur128`, no ffmpeg): duration, integrated loudness (LUFS), loudness range, true peak (dBTP), and `clipping` and `near_silent` flags. An agent cannot hear a song, so these numbers are its only objective check; the listener picks the take.

## Limits

- Jobs live in the MCP process. If it exits, running jobs are no longer tracked (the backend still finishes them); finished songs stay in `list_songs`.
- Takes run one after another on the backend, so 4 takes take about 4 times as long.
- mp3 output needs `ffmpeg` on the `serve` process's PATH; `flac` and `wav` do not.
- Retake and extend of a song section are not exposed yet.
