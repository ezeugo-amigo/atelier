# Lotus

An Elm frontend in a Tauri desktop shell, backed by typed Rust commands.

Lotus is the starting point for a native desktop email client. The first
version keeps the hard parts explicit:

- Elm owns the interface and all view state.
- A tiny JavaScript bridge turns Elm port messages into Tauri command calls.
- Rust exposes the backend API boundary for accounts, folders, messages, sync,
  read state, and archive actions.
- The current backend is local-first: an in-memory mailbox, mock OAuth-style
  providers, provider sync checkpoints, and a sync outbox make the account
  setup/read/archive flow runnable before adding IMAP, JMAP, real OAuth,
  keychain storage, or local indexing.

## Run It

You need:

- Elm 0.19.1
- A Rust toolchain
- Tauri CLI v2 (`cargo install tauri-cli --version "^2.0"`)

From this directory:

```sh
make dev
```

Useful targets:

```sh
make web      # compile src/Main.elm -> web/elm.js
make run      # build frontend, build Rust, run the debug binary
make dev      # Tauri dev mode
make build    # release Tauri bundle
make clean
```

## Shape

```text
lotus/
├── build.sh
├── docs/
│   ├── frontend-architecture.md
│   └── storage-architecture.md
├── elm.json
├── src/Main.elm
├── web/
│   ├── index.html
│   ├── tauri.js
│   └── styles/app.css
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    └── src/{main,lib}.rs
```

See [docs/frontend-architecture.md](docs/frontend-architecture.md) for the
frontend architecture and Tauri bridge flow.
See [docs/storage-architecture.md](docs/storage-architecture.md) for the
storage/provider boundary and SQLite schema.

The Rust command API is intentionally small and replaceable. Account setup
already runs through a provider registry with `MockGmail` and `MockOutlook`
adapters. The next real backend step is to replace those mock adapters behind
the same command surface:

- Account setup: OAuth for Gmail/Outlook, app passwords for generic IMAP.
- Storage: encrypted credentials in the OS keychain, message cache in SQLite.
- Transport: IMAP IDLE or JMAP push where available; SMTP or provider API for
  sending.
- Indexing: local full-text search over normalized messages.
