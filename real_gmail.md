# Real Gmail Integration Plan

## Goal

Replace Lotus's mock Gmail provider with a real Gmail integration while
preserving the existing Elm UI and provider/storage boundaries.

The first useful milestone is a connected Gmail account that can authenticate,
persist its credentials safely, import the Inbox, and render real messages in
the existing three-pane UI. Sending and remote mailbox mutations follow after
that vertical slice is stable.

## Current State

- `lotus/src-tauri/src/lib.rs` (1692 lines) contains `MockGmail` and
  `MockOutlook` providers, `MailStore`, `LocalFirstSyncEngine`, and all 13 Tauri
  commands in a single file.
- `MailProvider` exposes `option`, `begin_login`, `complete_login`, and
  `sync_mailbox`. All are synchronous `fn`. There are no mutation or send
  methods.
- `MailStore` is entirely in-memory: accounts, messages, sync state, outbox, and
  credentials.
- `lotus/src-tauri/migrations/001_mail_storage.sql` defines the intended SQLite
  schema, but nothing wires it into a storage layer.
- The Elm account flow asks for an email address (`Main.elm:364`) because the
  OAuth flow is simulated.
- The composer updates local UI state and does not send mail.
- `web/tauri.js` is a pure request/response bridge. It has **no**
  `window.__TAURI__.event.listen` call, so Rust cannot currently push to Elm.
- `tauri.conf.json` sets `"csp": null`.
- `capabilities/default.json` grants only `core:default`.
- Rust dependencies are `serde`, `tauri`, `time`. No HTTP client, OAuth, keychain,
  SQLite, MIME, or async runtime. No `[dev-dependencies]`.
- CI is one workflow, `.github/workflows/rustfmt.yml`. It runs `cargo fmt --check`
  over five crates and nothing else. No test, lint, or build gate exists.
- `time = "=0.3.47"` is in `Cargo.toml` but no source file references it. It was
  added in the initial scaffold commit (`72078b3`) with no explanation.

## Quality gate

Named here because a 24-day refactor that starts by splitting a 1692-line file
needs a machine-checkable phase boundary. Every phase's acceptance criteria mean
"these commands pass".

Add `.github/workflows/lotus-ci.yml`, filtered to `paths: ['lotus/**']`:

```text
lotus/build.sh                                    # elm make, the same script tauri.conf.json calls
cargo test    --manifest-path lotus/src-tauri/Cargo.toml
cargo clippy  --manifest-path lotus/src-tauri/Cargo.toml --all-targets -- -D warnings
npx elm-test --compiler ... (Phase 0 onward, see the golden fixture below)
```

A separate workflow rather than three more steps in `rustfmt.yml`, because
`rustfmt.yml` is repo-wide across five crates and the Lotus gate is
Lotus-scoped. `rustfmt.yml` keeps doing formatting only.

### The `time` pin

Verified: no source file mentions `time::` anything, and `cargo tree` resolves a
single `time v0.3.47` in the graph (reached through `cookie` and
`tauri-codegen`). The exact pin is not load-bearing and there is no conflict to
protect against.

Phase 0 replaces it with `time = { version = "0.3", features = ["formatting",
"parsing", "macros"] }`, which is what the ISO-8601 work actually needs. If a
`cargo update` later breaks the Tauri build, re-pin *with a comment saying why*.

### Mock conventions that real Gmail cannot satisfy

These four are the root cause of most of the work below. They are not
cosmetic. Each one crosses the Rust model, the SQLite schema, the wire shape,
and the Elm decoders simultaneously.

| Convention | Where | Why Gmail breaks it |
|---|---|---|
| `receivedAt` is display prose (`"Today 8:10 AM"`, `"Yesterday"`) | `lib.rs:52`, rendered verbatim at `Main.elm:966`, `Main.elm:1050` | Gmail gives `internalDate` epoch millis. The index `idx_mail_messages_account_received ... received_at DESC` only sorts correctly for ISO-8601. |
| Body is `body_paragraphs: Vec<String>`, escaped one-`<p>`-per-item | `lib.rs:74`, `Main.elm:1063` | Real mail is `multipart/alternative` with `text/html`, quoted chains, inline images. |
| One `folder_id` per message | `lib.rs:63`, archive overwrites it at `lib.rs:597` | Gmail messages hold many labels at once (`INBOX` + `IMPORTANT` + `CATEGORY_UPDATES` + user labels). The schema already models this correctly via `mail_message_folders`; the Rust model and wire shape do not. |
| Search is an in-memory `format!` haystack plus `to_lowercase().contains()` over every message | `lib.rs:614` | Fine for 3 seed messages. It is a full scan over every cached message once Phase 3 imports 5000. |

`syncStatus.lastChecked` (`"Just now"`), `outbox.created_at` (`"Now"`), and
`credential.expires_at` (`"1 hour from now"`) are prose for the same reason and
change with `receivedAt`.

### Search stays naive in v1

Decided, not deferred by accident. The current scan reads only header fields
(sender name, sender email, subject, snippet, labels) and never the body, so at
the Phase 3 cap of 5000 messages it touches roughly 5000 short strings per
keystroke-triggered search. That is single-digit milliseconds, and search is
already user-triggered through `RunSearch` rather than fired per keystroke.

FTS5 is the right answer at 50k messages. It is not the right answer at 5000,
and adding it in Phase 1 means maintaining an index and its triggers through
three more phases of schema churn. Revisit when the eviction cap in Phase 3
rises, and only then.

### Snippets are derived locally

`MessageSummary.snippet` (`lib.rs:51`) is hand-written in the mock seeds. Gmail
returns a `snippet` field on `messages.get`, but `format=RAW` does not include
it, and Decision 2 picks RAW.

Phase 3 derives the snippet from the parsed text body: collapse whitespace, take
the first 200 characters on a word boundary. A second `format=METADATA` call
would double the quota cost per message, and the rate-limit arithmetic in Phase 3
assumes exactly one fetch per message.

## Decisions Taken

Recorded here so no phase re-opens them.

1. **Timestamps are ISO-8601 UTC on the wire and in SQLite. Elm formats for
   display.** Requires promoting `elm/time` to a direct dependency and threading
   a `Time.Zone` from `Task.here` through `init`. Chosen over Rust-side
   formatting because the sort key and the display value must not diverge.
2. **v1 renders `text/plain` only, and HTML-only mail renders crudely.** When a
   message has no text part, strip tags and unwrap entities from the HTML part
   server-side. HTML rendering, inline images, and remote-content blocking are
   deferred to Phase 7. This keeps `bodyParagraphs` as the wire shape and avoids
   needing a sanitizer for the first release. See the cost note below: the
   fallback is the common path, not the rare one.
3. **`folderId` becomes `folderIds: List String`.** Chosen over a lossy
   primary-folder projection, because the projection needs a precedence rule that
   will still render wrong when a message is in both the inbox and a user label.
   Because a Gmail message carries `INBOX` and `CATEGORY_PROMOTIONS` at once, the
   unified-inbox pass in `messages_for_folder` (`lib.rs:478`) can now match the
   same message through several folders. It must dedupe by message id, or the
   list pane shows visible duplicate rows.
4. **Provider trait becomes `async`.** Network calls happen outside the storage
   lock; the lock is taken only to apply a delta.
5. **SQLite lands before the keychain.** Phase 2's durability criteria are
   unsatisfiable without it.
6. **The OAuth callback pushes to Elm via a Tauri event.** Requires new glue in
   `web/tauri.js` and a new Elm port. Chosen over Elm polling a status command
   because polling adds a timer and a request-kind for no benefit.
7. **No automatic retry for `send`.** A timeout can occur after Gmail accepted
   the message.
8. **Commands stay `async fn` and Tauri drives them.** Tauri 2 already runs a
   `tokio` runtime. No code calls `Runtime::new` or `block_on`, and the direct
   `tokio` dependency drops `rt-multi-thread`. A nested runtime panics at run
   time with "cannot start a runtime from within a runtime", and it would surface
   on the first real HTTP call in Phase 2 rather than at compile time.

### The cost of Decision 2

"v1 renders `text/plain` only" reads like a scope cut. It is only half a cut.
Marketing and transactional mail is very often HTML-only, so the fallback clause
is the common path.

HTML-to-text extraction that produces genuinely readable output needs
block-level awareness, list handling, link footnoting, and entity unwrapping.
That is 1 to 2 days, and it is not in the Phase 3 estimate.

The decision: **do not budget it. Accept crude output in v1.** Phase 3 does the
cheap version, roughly 2 hours: drop `<script>` and `<style>` subtrees, insert a
newline for each block-level close tag, collapse runs of whitespace, unwrap
entities, and drop everything else. Links lose their href. Nested lists lose
their structure. Tables read as runs of text.

This is honest because Phase 7 replaces the whole path with real HTML rendering
anyway, so a good extractor is work thrown away. The Phase 3 acceptance criteria
say so explicitly, and the release notes should too.

## Google Cloud Setup

*Est. 0.5 day, and it blocks Phase 2 — do it first, in parallel with Phase 0.*

1. Create or select a Google Cloud project.
2. Enable the Gmail API.
3. Configure the OAuth consent screen (External + Testing for development).
4. Create a **Desktop app** OAuth client.
5. Keep the downloaded client configuration out of Git.
6. Add the development Gmail account as a test user.
7. Request `gmail.modify` from the start — see the scope note below.

Local configuration:

```text
LOTUS_GOOGLE_CLIENT_CONFIG=/absolute/path/to/client_secret.json
```

The application must fail with a clear setup error when the configuration is
missing or malformed. No client ID or secret in source.

### Platform constraints to plan around

- **Refresh tokens expire after 7 days while the app is in Testing.** This is a
  Google policy, not a bug. Development accounts will need re-consent weekly
  until the app is verified. Phase 2's restart-durability criterion is worded
  around this. Start verification early if the release is public.
- **Request `gmail.modify` in Phase 2, not later.** Escalating
  `gmail.readonly` → `gmail.modify` invalidates the existing grant and forces
  re-consent. Asking for `modify` up front costs one extra consent line and saves
  a forced-reconnect migration between Phase 4 and Phase 5.
- **A Desktop-app client secret is not secret in a distributed build.** PKCE is
  what actually protects the flow. The env-var config solves development hygiene,
  not shipping. **Open question for release:** decide whether to ship the client
  ID embedded (normal for desktop OAuth) or route through a backend.

### The 0.5 day does not include Google's review queue

The 0.5 day covers the console clicks. It does not cover verification.

`gmail.modify` is a restricted scope. Publishing past Testing requires Google's
OAuth verification, which for restricted Gmail scopes includes a third-party
security assessment. That runs weeks, sometimes months, and it is outside the
team's control.

Nothing in Phases 0 through 6 blocks on it, so **development is safe**. The
public release blocks on it entirely. If the deadline is a public ship date
rather than an internal demo, verification is the critical path and it dwarfs the
24 to 33 day engineering estimate.

So: **file the verification request in week 1, in parallel with Phase 0.** It
costs an afternoon of forms and it runs in the background for the whole project.
The alternative, filing it in week 6 when the code is done, adds the entire
review latency to the end of the schedule for no reason.

If the first ship is internal only, Testing mode with up to 100 test users is
enough, and the 7-day refresh-token expiry is the price.

## Target Architecture

```text
Elm account UI
    -> commandOut port -> tauri.js -> Tauri commands
    <- commandIn port  <- tauri.js <- command responses
    <- eventIn port    <- tauri.js <- Tauri emit (OAuth callback, sync progress)
        -> OAuth session + loopback listener
        -> GmailProvider (async)
            -> Gmail HTTP client (reqwest)
            -> token manager
        -> keychain token store
        -> SQLite mail storage
        -> local-first sync engine
```

Tokens remain on the Rust side. Elm receives only account metadata, connection
status, sync status, and normalized mailbox data.

## Dependencies to Add

```toml
[dependencies]
# No rt-multi-thread: Tauri 2 owns the runtime. See Decision 8.
tokio = { version = "1", features = ["macros", "sync", "net", "time"] }
time = { version = "0.3", features = ["formatting", "parsing", "macros"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
rusqlite = { version = "0.32", features = ["bundled"] }
keyring = "3"
serde_json = "1"
mail-parser = "0.9"          # MIME parsing (Phase 3)
mail-builder = "0.4"         # MIME generation (Phase 6)
base64 = "0.22"              # urlsafe encode/decode for Gmail raw + PKCE
sha2 = "0.10"                # PKCE S256 challenge
rand = "0.8"                 # state + PKCE verifier
url = "2"
tauri-plugin-opener = "2"    # system browser launch

[dev-dependencies]
wiremock = "0.6"
tempfile = "3"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "test-util"] }
```

`rusqlite` with `bundled` is chosen over `sqlx` to avoid compile-time query
verification against a database that does not exist yet in CI.

Tauri ships plugins that cover two of these, and this plan deliberately does not
use them. Recorded so nobody re-litigates it in review. `tauri-plugin-sql`
exposes the database to the frontend, which breaks the rule that storage and
tokens stay Rust-side. `tauri-plugin-stronghold` would put a second secret store
beside the OS keychain the user already trusts, and the keychain is what gives us
"tokens never in our own files".

The `time` pin drops from `=0.3.47` to `0.3` with the formatting features. See
the quality-gate section above for why the exact pin is not load-bearing.

Also required, and easy to miss:

- `capabilities/default.json` must add `opener:default` (or a narrower
  `opener:allow-open-url`). It currently grants only `core:default`.
- `elm.json` must move `elm/time` from `indirect` to `direct`.

## File Layout

Splitting `lib.rs` is not optional at this scope — it grows past 4000 lines
otherwise.

```text
src-tauri/src/
  lib.rs              # Tauri builder, commands, AppState only
  model.rs            # Account, Folder, Message*, SyncStatus, wire types
  storage/
    mod.rs            # MailStorage trait
    sqlite.rs         # SqliteMailStorage
    memory.rs         # MailStore (kept for tests)
  provider/
    mod.rs            # MailProvider trait, ProviderRegistry
    mock.rs           # MockMailProvider
    gmail/
      mod.rs          # GmailProvider
      auth.rs         # loopback OAuth, PKCE, token refresh
      api.rs          # HTTP client, pagination, rate limiting
      mime.rs         # MIME parse + normalize
      labels.rs       # Gmail label <-> Lotus folder mapping
  sync.rs             # LocalFirstSyncEngine, outbox dispatch
  credentials.rs      # keychain-backed token store
```

`impl MailStorage for MailStore` (`lib.rs:909`) is 76 lines of pure delegation to
inherent methods of the same name, which is why call sites read as
`MailStorage::bootstrap(&*storage, ...)` with explicit path disambiguation
(`lib.rs:1322`). Collapse the inherent methods into the trait impl during the
split and delete the delegation layer. Otherwise the split copies 76 lines of
noise into `storage/memory.rs` and every call site keeps the awkward form.

### Elm splits too

`Main.elm` is 1495 lines. Phase 0 touches 14 sites in it, Phase 2 another 8,
Phase 6 two more. Splitting the Rust side for exactly that reason and leaving Elm
whole is inconsistent. Elm forbids cyclic imports, so the cut is forced to be
clean:

```text
src/
  Main.elm            # init, update, subscriptions, main
  Types.elm           # Account, Folder, Message*, SyncStatus, Model  <- lines 25-200
  Api.elm             # ports, request kinds, encoders, decoders      <- lines 1240-1330
  View/
    Sidebar.elm
    MessageList.elm
    Reading.elm
    Setup.elm
```

Half a day inside Phase 0. It is what makes the Phase 2 decoder churn
reviewable. Do it in the same pass as the Rust split, before any behavior
changes, so the diff is pure motion.

---

# Phase 0: Normalize the Data Model

**No network code. Fully testable against the existing mocks.** This exists so
that Phases 3-6 are not simultaneously fighting Gmail's API and Lotus's own
shapes.

*Est. 3-4 days.*

### Rust

- Split `lib.rs` per the layout above, and `Main.elm` per the Elm layout.
  Mechanical, do it first while the files are still small. Collapse the
  `MailStorage` delegation layer in the same pass.
- Repoint `time` to `0.3` with the formatting features, and add the Lotus CI
  workflow. Both are prerequisites for every acceptance criterion below.
- `received_at`: change all mock seed data to **fixed** ISO-8601 UTC literals.
  See the clock note below.
- `MessageSummary.folder_id` / `MessageDetail.folder_id` → `folder_ids:
  Vec<String>`.
- Rewrite `messages_for_folder` (`lib.rs:477-509`) to match on membership in
  `folder_ids`, **deduping by message id**. The unified-inbox branch collects
  every inbox-role folder, and one Gmail message can now match several of them.
  The `"starred"` role special-case can stay for now but note it becomes a normal
  label in Phase 3.
- Rewrite `archive_message` (`lib.rs:575-612`) to *remove* the inbox folder id
  rather than overwrite `folder_id`.
- Rewrite `recalculate_unread` (`lib.rs:891-906`) for many-to-many membership.
- Make `MailProvider` methods `async fn` (via `async-trait` or return
  `Pin<Box<dyn Future>>`). `MockMailProvider` implementations become trivially
  async.
- Make `SyncEngine::sync_once` async.
- **Narrow the storage lock.** Today `refresh_mail` (`lib.rs:1434-1437`) and
  `sync_pending_changes` (`lib.rs:1367`) hold `state.storage` across
  `provider.sync_mailbox`. Restructure to: read sync states under lock → drop
  lock → await provider → re-take lock → apply delta. Switch
  `Mutex<MailStore>` (`lib.rs:1314`) to `tokio::sync::Mutex` so it is not held
  across an await point. Commands stay `async fn`; nothing constructs a runtime.
- Replace prose in `SyncStatus.last_checked`, `SyncOutboxItem.created_at`, and
  `StoredCredential.expires_at` with ISO-8601 (or `Option<String>` where
  genuinely absent). `SyncStatus.state` and `.detail` stay prose — they are
  already display-only.
- Distinguish `Account.provider` (display label, `"Gmail"`) from the persisted
  discriminant (`ProviderKind`, `"gmail"`). Add `Account.provider_kind:
  ProviderKind` to the wire shape. The schema's
  `UNIQUE (provider, provider_account_id)` needs the stable machine value.

### Elm

- `elm/time` → direct dependency in `elm.json`.
- `Model` gains `timeZone : Time.Zone`; `init` runs `Task.perform GotTimeZone
  Time.here`.
- `MessageSummary.folderId : String` → `folderIds : List String`
  (`Main.elm:49`, `Main.elm:64`); update decoders at `Main.elm:1256`,
  `Main.elm:1272`, `Main.elm:1310`.
- `receivedAt : String` becomes `receivedAt : Time.Posix`; add a
  `formatReceivedAt : Time.Zone -> Time.Posix -> String` helper (relative for
  today/yesterday, absolute beyond). Used at `Main.elm:966` and `Main.elm:1050`.
- `currentFolderName` / folder filtering (`Main.elm:1103-1109`) updated for
  `folderIds`.

### Mock timestamps need a fixed clock

Converting `"Today 8:10 AM"` to ISO-8601 needs a reference instant, and the mock
seeds are constructed at call time inside `complete_login` (`lib.rs:1078`). If
the seeds call `SystemTime::now()`, the two existing tests become
non-deterministic and the third, which asserts on exact strings (`lib.rs:1605`),
breaks unpredictably.

Three options. Inject a `Clock` trait into `MockMailProvider` so tests pin the
instant. Compute seed timestamps as offsets from `now` and relax the string
assertions to shape assertions. Or hard-code fixed absolute literals.

**Take the fixed literals.** Mock mail is then dated `2026-07-01T12:10:00Z`
forever, which looks slightly odd in the mock UI and is otherwise free. A `Clock`
trait is the right design when production code needs a testable clock, and none
of it does: real timestamps come from Gmail's `internalDate`, not from our
clock. The only place we generate a time is `SyncStatus.last_checked`, and one
`OffsetDateTime::now_utc()` call at that site needs no abstraction.

### Land the wire-shape change atomically

Phase 0 renames `folderId` to `folderIds` and retypes `receivedAt`. Rust and Elm
ship in one binary, so there is no version skew to manage across releases. There
is a landing problem inside the branch: the Rust change and the Elm change must be
one commit per field, or the app is broken between them, and the breakage is
**silent**. `handleCommand` routes decoder failures into `model.error`
(`Main.elm:282`) rather than crashing, so a mismatch shows as an error banner
instead of a red test.

So: one commit per field, Rust plus Elm plus decoder together. Do not land
Rust-first. And add a golden-fixture test in both directions: serialize a
`BootstrapPayload` from Rust into `tests/fixtures/bootstrap.json` and assert the
bytes, then have an `elm-test` case decode that same file. A shape mismatch then
fails CI.

### Fix existing test

`lib.rs:1582` asserts `store.credentials.len() == 1`, reaching into private
state. It breaks when Phase 1 replaces `MailStore`. Replace with an assertion
through `MailStorage`.

### Acceptance criteria

- The Lotus CI workflow exists and is green: `build.sh`, `cargo test`,
  `cargo clippy --all-targets -- -D warnings`, `elm-test`.
- All three existing tests pass, adjusted only for the new shapes, and they are
  deterministic across runs.
- The golden bootstrap fixture round-trips Rust → JSON → Elm decoder.
- The app runs against mocks with identical visible behavior, except timestamps
  now render from real instants.
- A message can be in two folders at once, and archiving removes only the inbox
  membership.
- A message in `INBOX` and one user label appears exactly once in the unified
  inbox.
- No lock is held across an `.await`; no `Runtime::new` or `block_on` anywhere.

---

# Phase 1: SQLite Mail Storage

*Est. 3-4 days.*

- Implement `SqliteMailStorage` behind the existing `MailStorage` trait.
- Open the database under Tauri's application data directory
  (`app.path().app_data_dir()`).
- Run `migrations/001_mail_storage.sql` at startup. Add a `schema_version` table
  and a numbered runner, so migration `002` has somewhere to go when it is
  genuinely needed. Migration failure is fatal with an actionable message.
- Replace `MailStore` in `AppState` (`lib.rs:1448`); keep `memory.rs` for fast
  unit tests.
- Persist accounts, folders, messages, folder memberships, labels, recipients,
  sync checkpoints, outbox, and sync events.

### Edit `001` in place. There is no migration `002`.

`001` has never run anywhere. Nothing in `src/` references the migrations
directory, and no Lotus database exists on any machine (verified: no
`app_data_dir` entry). Writing a `002` that rebuilds `sync_outbox` to alter a
`CHECK` constraint is ceremony against a file no instance has executed.

Fold every change into `001` and ship one migration. This saves the
create-copy-drop-rename table rebuild, roughly half a day.

The one precondition: confirm no teammate has a local database from a feature
branch before editing. If someone does, delete their file. That is a cheaper
conversation than a permanent table-rebuild migration in the repo.

Changes to make in `001`:

- `mail_messages`: add `to_json`, `cc_json`, `reply_to_json`, `body_text`,
  `body_html`, `internal_date INTEGER` (epoch millis, for reliable sort).
  Explicit recipient columns replace relying on `body_json`.
- `sync_outbox`: change the status `CHECK` (line 85) to
  `IN ('pending','retryable','synced','failed')` and add `next_attempt_at TEXT`
  and `failure_kind TEXT`.
- Add `idx_sync_outbox_next_attempt ON sync_outbox(status, next_attempt_at)`.
- `mail_labels` table for Gmail label id → display name/colour.
  `mail_folders.provider_folder_id` already exists (line 36) and carries the
  Gmail label id.
- Change `idx_mail_messages_account_received` (line 114) to sort on
  `internal_date DESC`.

### Two `NOT NULL` columns break on real mail

`mail_messages.snippet TEXT NOT NULL` (line 53) and `sender_name TEXT NOT NULL`
(line 50) both forbid null. Real mail has unnamed senders, where `From` is a bare
address, and automated mail with an empty body yields no snippet. The upsert then
fails on a constraint violation mid-page, and Phase 3's "malformed MIME degrades
one message" handling will not catch it, because this is a database error rather
than a parse error.

Fix in `001`: default both to `''`. Phase 3 normalizes `sender_name` to the local
part of the address when `From` carries no display name.

### Deferred to Phase 7

Attachment metadata tables and thread-level metadata. Nothing in the first
release reads them.

### Acceptance criteria

- Restarting Lotus preserves accounts, folders, and messages.
- Existing mock-provider tests pass against `SqliteMailStorage` in a `tempfile`
  directory as well as `MailStore`.
- Command response shapes are byte-identical to Phase 0 output.
- Serving the last local snapshot still works with no provider reachable.
- A message with an empty snippet and a bare-address `From` inserts without
  error.
- The migration runner records `schema_version = 1` and is idempotent across
  restarts.

---

# Phase 2: Gmail OAuth + Credential Persistence

Merged, because keychain storage and account durability are one deliverable and
both need Phase 1.

*Est. 4-6 days. Highest-risk phase — the loopback flow and the Rust→Elm push are
both new mechanisms.*

### Rust — OAuth

- Add `ProviderKind::Gmail`; keep mocks registered.
- Load and validate `LOTUS_GOOGLE_CLIENT_CONFIG` at startup.
- Per login attempt: generate a 32-byte random `state` and a PKCE verifier;
  derive the S256 challenge.
- Bind a `tokio` TCP listener on `127.0.0.1:0`; read the assigned port for the
  redirect URI. **Bind before opening the browser** — see the loopback note.
- Build the redirect URI as `http://127.0.0.1:<port>` using the **IP literal,
  never `localhost`**. Google's Desktop-client docs steer to the literal, and the
  two are not interchangeable here: the hostname fails consent with
  `redirect_uri_mismatch`.
- Build the authorization URL with `access_type=offline`,
  `prompt=consent` (needed to reliably get a refresh token), and the
  `gmail.modify` scope.
- Open it with `tauri-plugin-opener`.
- Refuse to open a second concurrent session for the same provider. Return a
  clear error instead.
- On callback: validate `state` in constant time, exchange the code for tokens,
  respond to the browser with a small "you can close this" page, shut the
  listener down.
- Call `users.getProfile` for the email address and `historyId`. **Do not** ask
  the user to type an email.
- Expire pending sessions after 5 minutes; drop the listener.

### Loopback details that fail late if missed

Google permits a wildcard port for Desktop clients: it accepts any port on
`127.0.0.1`, so no per-port registration in the console. Binding `:0` and reading
the assigned port works.

Two things around it do not work by default. The IP-literal rule above is one.
The other is that macOS shows a firewall prompt the first time a signed app binds
a listening socket. If the browser is already open on Google's consent screen when
that prompt appears, it lands behind the browser, mid-flow, and looks like a
crash. **Bind the listener first, then open the browser**, so the prompt arrives
while the user is still looking at Lotus.

### Rust — credentials

- `keyring`-backed token store. Service key
  `ai.atelier.lotus.oauth.v1`, account key `gmail:<provider_account_id>`.
- Refresh tokens go to the keychain only. SQLite stores
  `credential_ref` (the keychain account key) — never a token value.
- Access tokens in memory, keyed by account, refreshed on 401 or when within 60s
  of expiry. Single-flight the refresh so concurrent requests do not stampede.
- On `invalid_grant`: mark the account disconnected, clear the in-memory token,
  keep cached mail, surface a reconnect action.
- Disconnect deletes the SQLite account row (cascades) and the keychain entry.
- `app_bootstrap` loads durable accounts from SQLite.

### Wire-up — the Rust→Elm push

`web/tauri.js` currently has no event listener. Add:

```js
window.__TAURI__.event.listen("lotus://event", (e) => {
  if (app.ports.eventIn) app.ports.eventIn.send(e.payload);
});
```

- New Elm port `eventIn : (Encode.Value -> msg) -> Sub msg`, wired into
  `subscriptions` (`Main.elm:441`).
- `begin_account_login` returns immediately after opening the browser, **and
  returns the session's `loginState`** so Elm can correlate the callback.
- `complete_account_login` **is removed** as an Elm-invoked command; completion
  arrives as a `lotus://event` payload. This is a real change to the command
  surface, not a shape-preserving one.

### Events need a correlation id

The command bridge carries a `requestId` and Elm tracks it in `model.pending`
(`Main.elm:180`). A `lotus://event` payload has no equivalent. A user who clicks
"Add account" twice, or abandons one consent flow and starts another, produces two
callbacks, and `SetupState` holds only one flow.

Every `lotus://event` payload carries `{ kind, loginState, ... }`. Elm stores the
`loginState` returned by `begin_account_login` in `WaitingForCallback` and
discards any event whose `loginState` does not match. Rust refuses a second
concurrent session for the same provider, so the two guards cover both the racing
and the stale case.

The OAuth `state` parameter is already a 32-byte random value scoped to exactly
one login attempt, so it is the natural correlation id. It never reaches a log
line, and Elm only compares it for equality.

### Elm

- `SetupState`: `MockLogin AccountLogin` → `OpeningBrowser ProviderKind`,
  `WaitingForCallback ProviderKind`, `LoginFailed String`. Keep `MockLogin` for
  the mock providers.
- Remove the email input and `LoginEmailInput` / `loginEmail` for Gmail
  (`Main.elm:349-370`, `Main.elm:780`). Mock providers keep it.
- Add `Reconnect` and `Disconnect` actions on an account row.
- Map provider errors to safe user-facing strings; never render a raw error that
  may contain request data.

### Acceptance criteria

- "Add account → Gmail" opens the system browser; no email is typed.
- Invalid, expired, replayed, or `state`-mismatched callbacks are rejected with a
  clear message and no account created.
- No access or refresh token appears in any Elm message, log line, or SQLite row
  (assert by grepping a test database).
- Restarting Lotus preserves the connected account without re-consent **for as
  long as Google honours the refresh token — 7 days while the app is in
  Testing.**
- Revoking access in Google produces a reconnect prompt, not a crash, and cached
  mail stays readable.
- Starting a second login while one is pending is refused, and a callback for an
  abandoned session is ignored rather than applied to the visible flow.
- The redirect URI uses `127.0.0.1`, and the firewall prompt (first run on macOS)
  appears before the browser opens.

---

# Phase 3: Initial Gmail Inbox Sync

*Est. 5-7 days. Largest phase; MIME normalization is where the long tail lives.*

- `users.labels.list` → map `INBOX`, `STARRED`, `SENT`, `DRAFT`, `TRASH`, `SPAM`
  to Lotus folder roles. Archive is *not* a Gmail label — it is the absence of
  `INBOX`; model the Archive folder as a local-only view.
- `users.messages.list` with `labelIds=INBOX`, following `nextPageToken`.
- `users.messages.get?format=RAW` in batches, parsed with `mail-parser`.
  (`format=FULL` avoids raw parsing but returns base64url part bodies you must
  reassemble anyway; RAW plus a real parser handles the malformed-header tail
  better.)
- Normalize: `From`/`To`/`Cc`/`Reply-To`, subject (RFC 2047 encoded words), the
  `text/plain` part, `internalDate` → epoch millis + ISO-8601. When `From` has no
  display name, set `sender_name` to the local part of the address.
- Derive `snippet` locally from the parsed text body: collapse whitespace, first
  200 characters on a word boundary. `format=RAW` does not return Gmail's own
  `snippet`, and a second `format=METADATA` call would double the quota cost.
- HTML-only messages go through the crude tag-stripper from Decision 2. Links
  lose their href and tables read as runs of text. That is accepted for v1.
- Preserve `id` as `provider_message_id` and `threadId` as
  `provider_thread_id`.
- Upsert transactionally per page: messages, folder memberships, labels.
- **`historyId` ordering:** capture `profile.historyId` *before* listing
  messages, then persist that pre-sync value after the import commits. Persisting
  the post-import value silently drops every change that landed during a long
  first import.
- Append a `provider_sync_events` row per pass.

### Rate limiting and batching

Not optional at inbox scale. Gmail allows 250 quota units/user/second, and
`messages.get` costs 5 units — an effective ceiling of ~50 fetches/second.

- Cap concurrency at 10 in-flight `messages.get` calls via a semaphore.
- Exponential backoff with jitter on 429 and 5xx; respect `Retry-After`.
- Treat 403 `rateLimitExceeded` as retryable, 403 `insufficientPermissions` as
  fatal-needs-reconsent.

### Initial sync behavior

- Import a bounded first window (500 most recent messages) synchronously, then
  continue pagination in a background pass **that stops at 5000 messages
  total**.
- Report progress through the existing sync status area via `lotus://event`.
- A failed page leaves already-imported messages readable.
- A malformed or unsupported MIME part degrades that one message (empty body,
  subject preserved) without failing the sync.

### The mailbox is capped at 5000 messages in v1

Without a bound, the background pass eventually holds the whole mailbox. A
ten-year Gmail account is 100k+ messages with full bodies, which puts the SQLite
file into the gigabytes. `mail_messages.deleted_at` exists in the schema (line
60) but no phase writes it.

Three options. Cap the import at a fixed count. Store bodies only for the most
recent N and headers for the rest. Or run a real LRU eviction keyed on
`deleted_at`.

**Take the fixed cap: 5000 messages, newest first.** Bodies-for-N-only means two
read paths, one of which has to fetch on demand and handle being offline, which is
Phase 3 work masquerading as a storage decision. LRU eviction needs an access
record that nothing writes yet.

The cap is honest about being a first release: 5000 messages is several months of
normal mail, search covers exactly what is stored, and raising the number later is
a constant. Say it in the UI: "showing your 5000 most recent messages."

- A newly connected account shows real Inbox messages.
- Sender, subject, snippet, received time, unread state, labels, recipients, and
  plain-text body all render correctly.
- Pagination works across more than one page.
- A mailbox with 5000+ messages completes the background pass without hitting a
  rate limit error that is not automatically recovered, and stops at the 5000
  cap.
- Refreshing while offline keeps the previous local mailbox visible.
- Unicode subjects and 8-bit encoded bodies render correctly.
- A message with no display name in `From` and no text body imports without a
  constraint error, and shows the address local part plus an empty snippet.
- An HTML-only marketing message renders as readable, ugly text. Not pretty is
  the criterion, not broken.

---

# Phase 4: Incremental Sync

*Est. 3-4 days.*

- `users.history.list?startHistoryId=<stored>`.
- Handle `messagesAdded`, `messagesDeleted`, `labelsAdded`, `labelsRemoved`.
- Re-fetch affected messages when a history entry lacks enough data.
- Apply unread / starred / folder-membership projections transactionally.
- **Drain the outbox to completion before applying any history page**, and skip
  history-driven label updates for any message that still has a pending or
  retryable outbox row. See the ordering note in Phase 5.
- Advance the checkpoint only after every change in the page is applied
  (`lib.rs:697`).
- On 404 (checkpoint too old for Gmail to replay) fall back to a bounded resync.
- Record retryable vs. permanent failures in `provider_sync_events`.

Manual refresh plus a periodic local timer. Gmail push/watch is deferred — it
needs a server-side Pub/Sub endpoint that a desktop-only app should not require.

### Acceptance criteria

- A message read in the Gmail web UI shows as read in Lotus after refresh.
- A message deleted remotely disappears locally.
- An invalid checkpoint triggers resync rather than an error state.
- Interrupting a history page mid-apply leaves the checkpoint un-advanced, and
  the next refresh replays it cleanly.

---

# Phase 5: Remote Mailbox Mutations

*Est. 3-4 days.*

Extend `MailProvider`:

```text
async fn mark_message_read(&self, account_id, provider_message_id, read)
async fn archive_message(&self, account_id, provider_message_id)
async fn star_message(&self, account_id, provider_message_id, starred)
```

All three are `users.messages.modify` calls: read/unread toggles the `UNREAD`
label, archive removes `INBOX`, starred toggles `STARRED`.

### The outbox actually has to work now

Two current gaps, both real:

- `sync_once` (`lib.rs:1292-1300`) marks pending items `"synced"` without
  contacting any provider. Add a dispatcher keyed on the `operation` string.
  `"account.connected"` (enqueued at `lib.rs:433-443`) is a no-op that should
  complete immediately.
- A failed item is unreachable forever: `mark_outbox_failed` sets `"failed"`
  (`lib.rs:817`) and `pending_outbox` only selects `"pending"` (`lib.rs:770`).
  Use the `'retryable'` status and `next_attempt_at` added to `001`, and select
  `status IN ('pending','retryable') AND next_attempt_at <= now`.

Classify failures: network/429/5xx → retryable with backoff; 401 → refresh then
retry once; 403 insufficient permissions / 404 message gone → permanent.
Cap attempts (5) then park as `failed` with an inspectable error.

- Apply the local mutation optimistically; reconcile from Gmail after success.

### Optimistic mutations and history replay fight each other

Phase 5 applies a local change immediately and queues an outbox item. Phase 4
applies remote history deltas to the same rows. If a refresh runs while an outbox
item is still pending, history replays the old label state over the optimistic
change and the message visibly flips back: the user marks something read,
refreshes, and watches it turn unread again.

The sync engine already runs outbox-first at the code level (`lib.rs:1264`) but
reads sync states before draining (`lib.rs:1270`), so the current shape has the
hazard.

Two rules fix it, and both are needed. First, `sync_once` drains the outbox to
completion, then reads sync state, then applies history. Second, the history
applier skips unread/starred/folder updates for any message with an unresolved
outbox row, because an item can be parked `retryable` for minutes and history
would win in that window.

The alternative, per-field version vectors, is correct in more cases and costs a
schema column plus reconciliation logic for a race that a single ordering rule
already closes.

### Acceptance criteria

- Marking read in Lotus changes Gmail.
- Archiving removes the message from the Gmail Inbox but keeps it locally in
  Archive.
- Killing the app mid-mutation leaves a `retryable` outbox row that drains on
  next launch.
- A permanent failure never erases the local cached message.
- Marking a message read and refreshing immediately does not flip it back to
  unread.

---

# Phase 6: Sending Mail

*Est. 2-3 days.*

- `send_message` Tauri command and provider method.
- Build RFC 5322 via `mail-builder`: To, Cc, Subject, `text/plain` body, optional
  `text/html`. Base64url-encode into `users.messages.send`'s `raw` field.
- Validate recipients and required fields before the network call.
- On success, insert the sent message locally or trigger a targeted sync.
- On failure, preserve composer contents and show an actionable error.
- **No automatic retry.** A timeout can occur after Gmail accepted the message.
  Surface it as "we could not confirm delivery — check Sent before resending."

### Elm

- Composer gains `Idle | Sending | Sent | Failed String` (`Main.elm:1067-1089`).
- `SendCompose` invokes the command instead of mutating local state.

### Acceptance criteria

- A plain-text message with Cc and a Unicode subject arrives correctly.
- A network failure keeps the draft in the composer.
- No code path retries a send automatically.

---

# Phase 7: Deferred

Not in the first release. Listed so scope creep is visible when it happens.

- **HTML body rendering.** Needs a sanitizer, a decision on `innerHTML` in Elm,
  remote-image blocking, and replacing `"csp": null` in `tauri.conf.json` with a
  real policy. `"csp": null` is fine for mock data and genuinely unsafe once
  sender-controlled markup reaches the DOM. **Tighten the CSP as part of this
  phase, before any HTML renders.**
- Attachments (metadata tables + cache).
- Draft autosave.
- Thread grouping.
- Gmail push/watch via Pub/Sub.
- Outlook/Graph provider.

---

# Testing Strategy

### Unit

- OAuth `state` and PKCE challenge generation/validation.
- Token expiry, refresh, single-flight, `invalid_grant` handling.
- Gmail label → Lotus folder mapping, including Archive-as-absence-of-INBOX.
- MIME parsing: RFC 2047 subjects, `multipart/alternative`, missing text part,
  malformed headers, 8-bit encodings.
- History event → mailbox delta conversion.
- Outbox failure classification, backoff, and attempt cap.
- MIME generation for plain text, HTML, Cc, Unicode subjects.
- Timestamp round-trip: `internalDate` → SQLite → wire → Elm formatting.

### Integration (`wiremock`)

- Pagination across multiple pages.
- Partial page failure leaves earlier pages imported.
- Invalid history checkpoint (404) → bounded resync.
- 429 with `Retry-After` → backoff and recovery.
- Revoked token mid-sync → account disconnected, cached mail intact.
- Malformed MIME degrades one message only.
- Migration `001` runs, is idempotent, and restart persistence works, in a
  `tempfile` app-data directory.
- A message with no `From` display name and an empty body upserts cleanly.
- Grep the test database for token-shaped values; assert none.
- The golden bootstrap fixture decodes in Elm (`elm-test`) and matches the Rust
  serializer byte for byte.
- Outbox-drains-before-history ordering: a pending mutation survives a history
  page that would revert it.
- Keep mock-provider tests as fast provider-independent coverage.

### Manual smoke

1. Start Lotus with a local Google client config.
2. Connect a test Gmail account; confirm no email was typed.
3. Confirm the Inbox appears with correct timestamps.
4. Restart; confirm account and cached messages remain.
5. Mark read, archive, refresh Gmail; verify both sides.
6. Change a label in the Gmail web UI; refresh Lotus; verify.
7. Send a test message to a controlled address.
8. Revoke access in Google; verify Lotus asks to reconnect and keeps cached mail.

---

# Scope Summary

One engineer, sequential. Ranges assume the Google Cloud setup is done in
parallel with Phase 0.

| Phase | Work | Est. |
|---|---|---|
| — | Google Cloud setup (console clicks only) | 0.5 d |
| — | File OAuth verification, week 1, runs in background | 0.5 d |
| 0 | Normalize data model, split `lib.rs` + `Main.elm`, async trait, narrow lock, CI gate | 4-5 d |
| 1 | SQLite storage (single migration `001`) | 3-4 d |
| 2 | OAuth loopback + keychain + event push + Elm states | 4-6 d |
| 3 | Initial Inbox sync + MIME + rate limiting | 5-7 d |
| 4 | Incremental sync | 3-4 d |
| 5 | Mutations + outbox dispatch + retry | 3-4 d |
| 6 | Send | 2-3 d |
| | Implementation total | 25-34 d |
| | Review round-trips, ~1 d per phase boundary | +4-6 d |
| | Manual smoke, 0.5 d after Phases 2, 3, 5, plus fixes | +3-4 d |
| | **Total** | **32-44 d (~7-9 weeks)** |

Phase 0 grew a day for the Elm split and the CI workflow. Phase 1 lost the
migration-002 table rebuild but gains the schema edits, so it nets flat.

Every implementation row is engineering time only. The review, QA, and fix rows
are new: the 8-step manual test is half a day per pass and wants running at the
end of Phases 2, 3, and 5, and nothing in the implementation estimate covers
fixing what those passes find.

**Say 7 to 9 weeks now rather than discovering it in week 6.** The ~20% buffer
often quoted for "test scaffolding and API surprises" is separate from review and
QA, and if this is the team's first Gmail integration, add it on top. If the
deadline cannot absorb the calendar, that is an argument for the cuts below, not
for a tighter estimate.

Phases 0 and 1 parallelize across two engineers only weakly, because Phase 1
depends on Phase 0's model changes.

## If the deadline is tighter than the estimate

Cut from the bottom, in this order. Each line is independently shippable.

1. **Drop Phase 6 (send).** Saves 2-3 d. Lotus becomes a read + triage client.
   Largest saving for the least structural damage.
2. **Drop Phase 4 (incremental sync).** Saves 3-4 d. Refresh does a bounded
   resync of the most recent N messages instead. Correct but wasteful of quota;
   acceptable for a demo, not for daily use.
3. **Drop `star` from Phase 5**, keep read + archive. Saves ~1 d.
4. **Narrow Phase 3's window to 100 messages** and skip the background
   pagination pass. Saves ~1-2 d.

**Do not cut Phase 0, 1, or 2.** Phase 0 is what keeps Phases 3-6 from becoming
a rewrite; skipping it moves cost later at a worse exchange rate. Phase 1 is
required for any restart durability. Phase 2 is the feature.

Absolute minimum credible slice: Phases 0 + 1 + 2 + 3, at **16-22 days
implementation, 20-27 with review and QA.** A Gmail account that connects,
persists, and renders a real Inbox read-only.

None of these cuts touch the OAuth verification queue. If the ship date is
public, that queue is the schedule, and cutting engineering work does not move
it.

# Definition of Done for the First Release

- A user connects Gmail through the system browser without typing an email.
- Tokens live in the OS keychain, never in SQLite, Elm, or logs.
- Accounts and cached mail survive restarts (within Google's Testing-mode
  refresh-token window).
- Real Inbox messages render with correct timestamps, recipients, labels, and
  plain-text bodies, up to the 5000-message cap.
- HTML-only mail renders as plain, ugly text rather than failing to render.
- Refresh performs incremental sync after the first import.
- Read and archive synchronize back to Gmail, with a draining retryable outbox.
- Sending a plain-text message works with no automatic retry.
- Gmail failures degrade to cached local mail with clear recovery actions.
- `csp` remains `null` **only** because no HTML body is rendered; Phase 7 must
  tighten it before that changes.

---

# Review Resolution Log

All 20 review comments from the annotated draft, and where each one landed.

| # | Severity | Resolution |
|---|---|---|
| 1 | process | New "Quality gate" section. A Lotus-scoped CI workflow, separate from the repo-wide `rustfmt.yml`. Verified: `rustfmt.yml` was the only workflow. |
| 2 | risk | Verified with `cargo tree`: no source file uses `time`, and the graph resolves one `time v0.3.47`. The pin is not load-bearing. Phase 0 repoints it to `0.3` with formatting features. |
| 3 | gap | Added to the blocking-conventions table as a fourth row, with an explicit decision: search stays naive in v1. The 5000-message cap makes a full scan cheap, and FTS5 through three phases of schema churn is not worth it. |
| 4 | gap | Snippet derives locally from the parsed body. Stated in the blocking-conventions section and in Phase 3. |
| 5 | blocker | Not budgeted. Decision 2 now says HTML-only mail renders crudely, roughly 2 hours of tag stripping, because Phase 7 replaces the whole path. Phase 3's acceptance criteria say "readable, ugly". |
| 6 | design | Decision 3 and Phase 0 both call for deduping by message id in `messages_for_folder`. New acceptance criterion. |
| 7 | blocker | New section under Google Cloud setup. File verification in week 1. Added as its own row in the scope table, and named as the real critical path for a public ship. |
| 8 | simplify | Two sentences in the dependency section on why `tauri-plugin-sql` and Stronghold lose. |
| 9 | risk | New Decision 8. `rt-multi-thread` dropped from the feature list; no `Runtime::new` or `block_on`; added to Phase 0's acceptance criteria. |
| 10 | mechanical | The file-layout section says to collapse the 76-line delegation layer during the split. |
| 11 | gap | New "Elm splits too" section with a concrete four-module cut. Phase 0 grew a day for it. |
| 12 | blocker | New "Land the wire-shape change atomically" section. One commit per field, plus a golden bootstrap fixture asserted from both sides so a mismatch fails CI instead of showing a banner. |
| 13 | design | Fixed absolute timestamp literals in the mock seeds. A `Clock` trait is rejected: no production code needs a testable clock. |
| 14 | simplify | Verified no Lotus database exists on disk and nothing in `src/` reads the migrations directory. Migration `002` is deleted from the plan and folded into `001`. The numbered runner still ships, so a future `002` has somewhere to go. |
| 15 | correctness | `snippet` and `sender_name` default to `''` in `001`. Phase 3 falls back to the address local part. New criteria in Phases 1 and 3. |
| 16 | blocker | Phase 2 now specifies the `127.0.0.1` literal explicitly and binds the listener before opening the browser, so the macOS firewall prompt lands inside Lotus. |
| 17 | correctness | Every `lotus://event` payload carries `loginState`, which is the OAuth `state`. Elm discards mismatches. Rust refuses concurrent sessions per provider. |
| 18 | design | Fixed cap: 5000 messages, newest first, surfaced in the UI. Bodies-for-N-only and LRU both rejected as more machinery than a first release needs. |
| 19 | correctness | Two ordering rules in Phase 5, referenced from Phase 4: drain the outbox to completion first, and skip history label updates for messages with unresolved outbox rows. Version vectors rejected. |
| 20 | process | Scope table gains review and QA rows. Total is now 32 to 44 days, 7 to 9 weeks. |
