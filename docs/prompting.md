# Prompting

## Style tags (`--style-file` or the positional argument)

Comma-separated genre, mood, instruments, vocal type, and tempo or era. Specific tags give better control.

```
dream pop, breathy female vocals, reverb guitars, synth pads, 90 bpm, melancholic
1990s boom bap hip hop, male rapper, dusty drums, jazzy sample
epic orchestral, choir, taiko drums, cinematic
```

Describe the style with tags rather than artist names. Save tags you like in a file under `styles/` (one line is enough) and pass it with `--style-file`. The repo ships styles for the songs made so far; any style can be paired with any lyrics.

## Lyrics (`--lyrics-file`)

Put a section tag on its own line, then the words.

```
[verse]
Walking down the street in the morning light
[chorus]
Everything is gonna be alright
```

Tags: `[intro]`, `[verse]`, `[pre-chorus]`, `[chorus]`, `[bridge]`, `[outro]`, `[instrumental]`.

- Write the lyrics to a file in `lyrics/` and pass it with `--lyrics-file`. `--lyrics "..."` exists for quick inline tests.
- Keep lines short and singable, with similar line lengths per verse.
- Match lyric amount to duration. Two verses and a chorus need roughly 90 to 150 seconds.
- For no vocals, put `[instrumental]` in the lyrics file.

## Iterating

- Each run uses a random seed. Generate several takes and keep the best, or pin one with `--seed`.
- Change one thing at a time: a tag, `--bpm`, or a lyric line.
- `--bpm` (30-300) and `--key` (for example `"A minor"`) pin tempo and key.

## Backend options not exposed

ACE-Step supports more fields (`time_signature`, `vocal_language`, `batch_size`, `sample_mode`, `lm_temperature`, `use_cot_caption`, ...). They are documented in `ACE-Step-1.5/.claude/skills/acestep-docs/api/API.md`. `musicai` has no flag for them, but the REST API and the MCP `create_song` tool forward them through the `advanced` object.

## Tuning notes

Findings from listening tests on a 60 s song (XL SFT model, 4B LM, seed 42 unless noted). They come from one listener and a few seeds, so treat them as starting points.

- **Guidance:** `--guidance-scale 4` sounded cleaner and more human than the default 7. It held on three seeds (42, 7, 99). High guidance (7 and 10) put a "weight" on the vocals, while 4 stayed clean. Values 2 and 3 were also generated; 5 and 6 are not yet judged. Guidance steers each step towards your tags and lyrics, so more is not better. Turbo models ignore it.
- **Tempo and key:** with `bpm` and `key` unset, the LM picks different ones on every run, even with a fixed seed (for example 200 BPM B-flat major, then 48 BPM C-sharp major, for the same prompt). Pin them to the style's own values.
- **The LM rewrites your style text** by default (`use_cot_caption`), and the rewrite can add things you did not ask for, such as "gated reverb" and layered harmonies. Pass `advanced: {"use_cot_caption": false}` in the MCP, or run the backend with the rewrite off, to send the text unchanged.
- **Speed:** XL SFT at 50 steps takes about 4 minutes for a 60 s song on an M5 Pro, roughly three times turbo.
- **Lyrics for power:** the ACE-Step docs say UPPERCASE gives stronger vocal intensity, parentheses mark background vocals, one descriptor per section tag works better than stacking (`[chorus - powerful]`), and 6-10 syllables per line align best with the beat. `lyrics/agentic-loop-rock.txt` applies these. With the `hard-rock` style it gave the best hard-rock take in listening (`hr2-g4-s7`), better than the same style with the longer-lined `agentic-loop` lyrics.

### Recipes that sounded best

All: XL SFT (`acestep-v15-xl-sft`), 50 steps, 4B LM, `--guidance-scale 4`, 60 s, flac.

| Style | Lyrics | bpm / key | Seed | Notes |
| --- | --- | --- | --- | --- |
| `hard-rock` | `agentic-loop-rock` | 116 / A major | 7 | Best rock take so far |
| `new-wave` | `agentic-loop` | 168 / E minor | 42 | Clean and crisp; also fine with seeds 7 and 99 |

Use `musicai generate --style-file styles/<style>.txt --lyrics-file lyrics/<lyrics>.txt --model acestep-v15-xl-sft --lm-model acestep-5Hz-lm-4B --steps 50 --guidance-scale 4 --bpm <bpm> --key "<key>" --seed <seed> -f flac -d 60`. `musicai serve` must have been started with the same `--model` and `--lm-model`.

### Open: vocals still sound slightly off

The best takes still have a small unexplained oddness in the vocals. Untested ideas, one change at a time from the best take:

- **Plain lyrics.** Words like "sub-agents", "repo" and "tool calls" may be sung oddly.
- **No tiled VAE decode.** `advanced: {"use_tiled_decode": false}`. The API decodes in overlapping chunks by default (512 latent frames, 64 overlap), so seams could sit near 15, 31 and 46 s of a 60 s song.
- **Adaptive Dual Guidance.** `advanced: {"use_adg": true}` (base and sft models).
- **Guidance only for part of sampling.** `advanced: {"cfg_interval_end": 0.8}`.
- **Alternative VAE.** `musicai serve --vae scragvae`; one listening run was inconclusive and the log never confirmed the VAE it loaded.

DCW (wavelet correction) is not a suspect: the API turns it off for sft models because forcing it on distorts the audio.
