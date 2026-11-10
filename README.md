# music-ai

Local AI song generation on Apple Silicon. A small Rust CLI (`musicai`) that serves a REST API and generates songs on top of [ACE-Step 1.5](https://github.com/ACE-Step/ACE-Step-1.5) (MIT). Minimum: an M1 or newer Mac with 16 GB RAM (see [Setup](docs/setup.md#requirements)). Tested on an M5 Pro with 48 GB.

## Quick start

Requires [mise](https://mise.jdx.dev), [uv](https://docs.astral.sh/uv/) and a Rust toolchain.

```sh
mise install        # mpv, ffmpeg
mise run setup      # clone pinned ACE-Step backend, install its Python env

# terminal 1: start the server (launches the ACE-Step backend too)
mise run serve

# terminal 2: write lyrics to a file, then generate a song from it
cat > docs/lyrics/late-night.txt <<'EOF'
[verse]
Late night city lights
[chorus]
We are alive tonight
EOF
mise run generate -- "dream pop, breathy female vocals, 90 bpm" --lyrics-file docs/lyrics/late-night.txt -d 60

# play it
mpv output/
```

Other tasks: `mise tasks` lists them; `mise run test` runs clippy and the tests. Without mise, the same commands are `scripts/setup.sh`, `cargo run --release -- serve` and `cargo run --release -- generate ...`.

## Commands

| Command | Purpose |
| --- | --- |
| `musicai serve` | Start the REST API on `127.0.0.1:8000` and the ACE-Step backend on `:8001` |
| `musicai generate <prompt>` | Submit a song, wait, save the audio to `output/`, print the path |

`musicai generate` flags: `--lyrics-file`, `-d/--duration` (10-600 s, default 60), `--bpm`, `--key`, `--seed`, `-f/--format` (`mp3` default, `flac`, `wav`), `-n/--name`, `-o/--out`, `--server`, and `--lyrics` for inline text (prefer a lyrics file). Run `musicai <command> --help` for details.

Keep each song's lyrics in a file under `docs/lyrics/`, for example `docs/lyrics/late-night.txt` with `[verse]` and `[chorus]` section tags (see [Prompting](docs/prompting.md)). Output files are named from `--name`, else from the `--lyrics-file` name (`docs/lyrics/late-night.txt` becomes `output/late-night.mp3`), else `<timestamp>-<id>`. An existing file is never overwritten; a repeat run gets `-2`, `-3`, and so on.

## REST API

| Method | Path | Result |
| --- | --- | --- |
| `POST` | `/v1/songs` | `202 {"id","status":"running"}`; body: `prompt`, `lyrics`, `duration`, `bpm`, `key_scale`, `seed`, `format` |
| `GET` | `/v1/songs/{id}` | `{"id","status":"running\|succeeded\|failed","error"}` |
| `GET` | `/v1/songs/{id}/audio` | Audio bytes; `409` until the song has succeeded |
| `GET` | `/health` | `ok` |

```sh
curl -s localhost:8000/v1/songs -H 'Content-Type: application/json' \
  -d '{"prompt":"lofi hip hop, mellow piano","lyrics":"[instrumental]","duration":30}'
```

## Docs

- [Setup](docs/setup.md): install, first run, mp3 and ffmpeg, troubleshooting
- [Prompting](docs/prompting.md): style tags, lyrics structure
- [Examples](docs/examples.md): full prompts that have been run

## License

MIT, see [LICENSE](LICENSE). The ACE-Step backend is a separate project under its own MIT license.
