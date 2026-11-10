# Prompting

## Style tags (the positional argument)

Comma-separated genre, mood, instruments, vocal type, and tempo or era. Specific tags give better control.

```
dream pop, breathy female vocals, reverb guitars, synth pads, 90 bpm, melancholic
1990s boom bap hip hop, male rapper, dusty drums, jazzy sample
epic orchestral, choir, taiko drums, cinematic
```

Describe the style with tags rather than artist names.

## Lyrics (`--lyrics-file`)

Put a section tag on its own line, then the words.

```
[verse]
Walking down the street in the morning light
[chorus]
Everything is gonna be alright
```

Tags: `[intro]`, `[verse]`, `[pre-chorus]`, `[chorus]`, `[bridge]`, `[outro]`, `[instrumental]`.

- Write the lyrics to a file in `docs/lyrics/` and pass it with `--lyrics-file`. The file name also becomes the song's file name. `--lyrics "..."` exists for quick inline tests.
- Keep lines short and singable, with similar line lengths per verse.
- Match lyric amount to duration. Two verses and a chorus need roughly 90 to 150 seconds.
- For no vocals, put `[instrumental]` in the lyrics file.

## Iterating

- Each run uses a random seed. Generate several takes and keep the best, or pin one with `--seed`.
- Change one thing at a time: a tag, `--bpm`, or a lyric line.
- `--bpm` (30-300) and `--key` (for example `"A minor"`) pin tempo and key.

## Backend options not exposed

ACE-Step supports more fields (`time_signature`, `vocal_language`, `batch_size`, `sample_mode`). They are documented in `ACE-Step-1.5/.claude/skills/acestep-docs/api/API.md` and are not passed through by `musicai`.
