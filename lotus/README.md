# Lotus

An Elm frontend in a Tauri desktop shell, backed by typed Rust commands.

Lotus is a native desktop email client. It connects a real Gmail account through
the system browser, stores its credentials in the OS keychain, and caches the
inbox in SQLite so mail stays readable offline.

- Elm owns the interface and all view state.
- A small JavaScript bridge turns Elm port messages into Tauri command calls, and
  forwards Tauri events back.
- Rust owns accounts, storage, providers, and sync. Tokens never leave it.
- Mock providers remain registered beside Gmail, so the UI and the sync engine
  stay testable with no network.

## Connect Gmail

Gmail needs a Google Cloud OAuth client. This is one-time setup.

1. Create or select a project at [console.cloud.google.com](https://console.cloud.google.com).
2. Enable the **Gmail API**.
3. Configure the OAuth consent screen: External, publishing status Testing.
4. Add your own Gmail address under **Test users**. Testing mode allows up to 100.
5. Create an OAuth client of type **Desktop app** and download the JSON.
6. Keep that file out of Git.

Point Lotus at it:

```sh
export LOTUS_GOOGLE_CLIENT_CONFIG=/absolute/path/to/client_secret.json
make dev
```

Then click **Add Account → Gmail**. The system browser opens, you consent, and
the tab tells you to come back. Lotus reads your address from Gmail's own
profile response, so there is no email field to fill in.

Two things worth knowing up front.

**Refresh tokens expire after 7 days while the app is in Testing.** That is
Google policy, not a bug. Development accounts need re-consent weekly. Cached
mail stays readable the whole time; only syncing stops.

**`gmail.modify` is a restricted scope.** Publishing past Testing requires
Google's OAuth verification, which for restricted Gmail scopes includes a
security assessment. That takes weeks and is outside your control, so file it
early if you plan to ship publicly.

Without `LOTUS_GOOGLE_CLIENT_CONFIG` set, Lotus still runs: Gmail is absent from
the provider list and the mock providers work as before.

## What works today

Phases 0 through 3 of the integration plan.

- Gmail sign-in through the system browser, PKCE on a `127.0.0.1` loopback port.
- Refresh tokens in the OS keychain. SQLite stores only a keychain reference.
- The inbox imports 500 messages synchronously, then paginates in the background
  to a cap of 5000.
- Messages render with real sender, subject, snippet, timestamp, recipients,
  labels, and plain-text body. HTML-only mail renders as stripped text.
- Accounts, folders, and messages survive a restart.
- Refresh re-reads the first inbox page, so remote read and label changes land.
- Mark-read and archive change local state and queue an outbox item.

Not yet: sending, remote mutations reaching Gmail, incremental `history.list`
sync, HTML rendering, attachments, threads. Those are Phases 4 through 7 of
[the plan](../real_gmail.md).

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

## Tests

```sh
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
```

The suite covers OAuth state and PKCE, the loopback callback over a real socket,
MIME parsing across encoded subjects and HTML-only bodies, Gmail label mapping,
rate-limit failure classification, and both storage backends behind the same
assertions.

Two guards are worth knowing about. `tests/oauth_loopback.rs` drives the callback
listener over TCP, because that path involves a socket and a hand-parsed HTTP
request that unit tests would miss. `tests/wire_shape.rs` pins the Rust-to-Elm
JSON contract against a golden fixture and greps `src/Api.elm` for the fields it
decodes, because an Elm decoder failure surfaces as a UI banner rather than a
test failure. Regenerate the fixture with:

```sh
LOTUS_BLESS_FIXTURE=1 cargo test --test wire_shape
```

## Shape

```text
lotus/
├── build.sh
├── docs/
│   ├── frontend-architecture.md
│   └── storage-architecture.md
├── elm.json
├── src/
│   ├── Main.elm          # init, update, subscriptions
│   ├── Types.elm         # model and wire types
│   ├── Api.elm           # ports, decoders, ISO-8601 parsing
│   └── View/             # Sidebar, MessageList, Reading, Setup, Common
├── web/
│   ├── index.html
│   ├── tauri.js          # command bridge + lotus://event listener
│   └── styles/app.css
└── src-tauri/
    ├── migrations/001_mail_storage.sql
    ├── tests/            # oauth_loopback, wire_shape
    └── src/
        ├── lib.rs        # Tauri builder, commands, AppState
        ├── model.rs      # wire and domain types
        ├── credentials.rs
        ├── sync.rs
        ├── storage/      # MailStorage trait, sqlite, memory
        └── provider/     # MailProvider trait, mock, gmail/
```

See [docs/frontend-architecture.md](docs/frontend-architecture.md) for the
frontend architecture and Tauri bridge flow.
See [docs/storage-architecture.md](docs/storage-architecture.md) for the
storage/provider boundary and SQLite schema.

## Conventions that are load-bearing

Three decisions cross the Rust model, the SQLite schema, the wire shape, and the
Elm decoders at once. Changing any of them means changing all four.

**Timestamps are ISO-8601 UTC everywhere except the screen.** Elm parses and
formats for display. Rust-side formatting was rejected because it makes the sort
key and the displayed value diverge.

**A message belongs to many folders.** `folderIds` is a list, because a Gmail
message carries `INBOX` and `CATEGORY_PROMOTIONS` and user labels
simultaneously. Archive is the absence of `INBOX`, not a label, so it is a
local-only folder.

**Tokens live in the keychain and nowhere else.** SQLite holds a
`credential_ref`. A test greps a real database file for token-shaped values and
asserts none.
