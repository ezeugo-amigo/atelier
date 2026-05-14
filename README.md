# Atelier

> A workshop of small, sharp tools for working with AI coding agents.

Atelier is a collection. Each tool is self-contained — its own README, its own
dependencies, its own way of being run — but they share a sensibility: keep the
human in flow, let the agents do the work, surface what needs attention and
hide what doesn't.

## Tools

- **[vigil/](vigil/)** — a native terminal dashboard that watches your Claude
  Code, Codex, Pi, and OpenCode sessions in parallel, and routes your attention
  to the one that needs you right now. See
  [vigil/docs/architecture.html](vigil/docs/architecture.html) for the design.

- **[diffdesk/](diffdesk/)** — a terminal-launched Tauri desktop diff reviewer
  with inline comments and AI-ready export. Open a working-tree diff, a commit
  range, or a patch file in a polished UI; annotate; ship the review as
  Markdown or JSON.

(More tools as they exist.)

## Why

Working with multiple agents fragments attention. One session is awaiting a
permission prompt; another is running quietly; a third finished an hour ago
and you forgot. The tools in Atelier exist so the right session gets in front
of you at the right moment, and the work between sessions feels less like
context-switching and more like flow.

## Repo layout

```
atelier/
├── README.md          (this file)
├── LICENSE            (MIT, applies to the whole repo)
├── vigil/             (one tool)
│   ├── Cargo.toml     (its own Cargo workspace)
│   ├── crates/
│   └── docs/
└── diffdesk/          (another tool)
    ├── Cargo.toml     (its own Cargo workspace)
    ├── src/
    └── src-tauri/
```

There is no top-level Cargo workspace; each tool builds independently. `cd`
into the tool you want to work on.

## License

MIT — see [LICENSE](LICENSE).
