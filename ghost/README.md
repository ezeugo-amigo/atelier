# Ghost

> A quiet typing test for macOS that races you against your own best run.

Ghost is a typing test in the spirit of Monkeytype, set in a bookish serif on warm paper. Its one
big idea: every personal best is recorded keystroke by keystroke, and the next time you play that
mode, a second caret (your ghost) replays that run alongside you. You can see, letter by letter,
whether you're ahead of your past self.

## Features

- **Your ghost.** A blue caret replays your best run in the current mode, including its
  hesitations and corrections. The meta line shows how many letters you're ahead or behind.
  Beat it and your new run becomes the ghost. Prefer an even pace? Switch the ghost to
  *steady pace* (your best's average speed) or turn it off in your profile.
- **Modes.** Timed (15, 30, 60, 120 seconds) or word count (10, 25, 50, 100 words), drawn from
  the 200 most common English words. Each mode keeps its own best and its own ghost.
- **Results.** WPM, accuracy, raw speed, character breakdown, and a chart of this run's speed
  against your best's, second by second.
- **Profiles.** Several people can share one computer. Each profile keeps its own results,
  bests, ghosts, and theme, and can be locked with an optional PIN.
- **Two themes.** *Paper* (light) and *cocoa* (dark).

## Keyboard

| Key | Action |
| --- | --- |
| any letter | starts the test |
| Space | next word |
| Backspace | delete a letter; on an empty word, step back into a mistyped previous word |
| ⌥ / ⌃ Backspace | delete the whole word |
| Tab / Esc / ⌘R | restart with new words, even mid-test |
| Tab / Enter / ⌘R (results) | next test |
| Esc (profile) | back to typing |

## How the score works

WPM counts the letters of correctly typed words plus the spaces after them, divided by five, per
minute. A word you're midway through when time runs out counts as far as it's right. Raw counts
every character you typed. Accuracy is correct keystrokes over all keystrokes.

## Run it

```sh
make install   # build Ghost.app and copy it to /Applications
make launch    # open it
make dev       # Tauri dev mode with hot reload
make test      # typecheck + unit tests
```

Needs [Bun](https://bun.sh) and a Rust toolchain. `bun run vite` serves the frontend alone at
http://127.0.0.1:1430, storing data in localStorage instead of on disk.

## Data

Everything lives in one JSON file: `~/Library/Application Support/ai.atelier.ghost/ghost.json`.
Each profile keeps its last 1,000 results; personal bests (with their keystroke traces) are kept
for good. A PIN keeps people on the same computer out of each other's profiles; it isn't
encryption, and the file is readable by anyone with access to your user account.

## Layout

```
ghost/
├── src/
│   ├── engine.ts      typing test state, scoring, keystroke trace (no DOM)
│   ├── ghost.ts       where the ghost is at time t (replay or steady)
│   ├── store.ts       profiles, results, bests, persistence
│   ├── chart.ts       results chart
│   ├── views/         profiles, test, results, profile screens
│   └── main.ts        app shell and header
├── test/              bun unit tests
└── src-tauri/         Tauri shell; load_db / save_db commands
```
