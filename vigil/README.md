# Vigil

> One quiet watcher for every coding agent.

Vigil is a native terminal dashboard that watches your Claude Code, Codex,
OpenCode (and the next agent) sessions in parallel — and tells you which one
needs you right now.

## Why

Agents wait on you more than you wait on them. The bottleneck is no longer
model speed — it's **human attention routing**. When five sessions are
running, four are idle and one is blocked on a permission prompt you can't
see, you lose minutes per context switch and hours per day.

Vigil makes the fleet legible. It reads every running agent's session log,
classifies what state each is in (running, awaiting input, idle, done), and
surfaces only the ones that need a human.

## Design principles

- **Read-only by default.** Vigil observes agent files; it never writes to
  them. Attaching to a session shells out to the agent's own CLI.
- **Agent-agnostic core.** The UI never knows what kind of agent it is
  rendering. Adapters do the translating.
- **Local first.** Everything lives on disk. No daemon, no server, no auth.
  v1 is one binary.
- **Honest about uncertainty.** State classification is heuristic. When
  Vigil isn't sure, the UI says so rather than guessing wrong.

## Architecture

See [`docs/architecture.html`](docs/architecture.html) for the full design
sketch — diagrams, the `AgentAdapter` trait, the state model, the event
loop, the crate layout, and open questions.

Open it directly in a browser:

```sh
open docs/architecture.html
```

## Status

Architecture sketch. No working code yet. The Cargo workspace skeleton is
in place so `cargo check` runs from day one; each crate is currently a
stub with a doc comment pointing back to the architecture.

## Roadmap

1. Claude Code adapter + classifier, validated against real session files.
2. Minimal `ratatui` shell rendering the registry as a flat list.
3. Attach to a session with `↵`.
4. Codex adapter (second proof the trait is right).
5. OpenCode adapter.

## License

MIT — see [`LICENSE`](LICENSE).
