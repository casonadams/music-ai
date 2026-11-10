# Examples

Every song so far was generated for 150 seconds with `musicai generate --style-file styles/<style>.txt --lyrics-file lyrics/<lyrics>.txt -d 150`. A 150 second song took about 80 to 110 seconds on an M5 Pro with 48 GB.

| Lyrics | Style | Notes |
| --- | --- | --- |
| `ai-token-usage` | `pop-punk` | Token costs through the SDLC |
| `sdlc-shift-left` | `celtic-punk` | "Ship it left", a play on shift left |
| `agentic-loop` | `hard-rock` | Agents in the loop |
| `agentic-middle` | `emo-pop-punk` | Hang on, you are in the middle of the sprint |
| `agentic-ship-easy` | `acoustic-rock` | One small commit at a time |
| `agentic-run-it-again` | `new-wave` | Run it again |
| `agentic-machine-loop` | `industrial-rock` | You decide what ships |

The lyrics were each written for the feel of the style they were first paired with, but any lyrics work with any style. For example:

```sh
musicai generate --style-file styles/new-wave.txt --lyrics-file lyrics/agentic-loop.txt -d 150
```

writes `output/agentic-loop-new-wave.mp3`.

Each style was chosen by describing a well-known song's feel with tags, without naming the artist. Lyrics are original.
