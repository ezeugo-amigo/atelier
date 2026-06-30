# Lotus Frontend Architecture

Lotus is an Elm application running inside a Tauri webview. Elm owns the UI
and view state. Rust owns mailbox data and mutations. A small JavaScript bridge
connects Elm ports to Tauri commands.

## File Map

```text
lotus/
├── src/Main.elm              Elm app: model, update loop, views, decoders
├── web/index.html            Static shell loaded by Tauri
├── web/tauri.js              Elm port bridge to Tauri invoke
├── web/styles/app.css        Layout and component styling
└── src-tauri/src/lib.rs      Rust command API consumed by the frontend
```

## Runtime Flow

```text
Tauri window
  -> web/index.html
    -> elm.js
    -> web/tauri.js
      -> Elm.Main.init(...)

User action in Elm
  -> update
    -> enqueue command request
      -> commandOut port
        -> tauri.js
          -> window.__TAURI__.core.invoke(...)
            -> Rust command
              -> JSON response
          -> commandIn port
    -> decode response
    -> update Elm model
    -> rerender view
```

The important boundary is this:

- Elm does not call Rust directly.
- Rust does not know about frontend widgets.
- JavaScript does not own application state.
- Tauri commands are the API contract between the UI and backend.

## HTML Shell

`web/index.html` is intentionally thin:

- Creates `<div id="root"></div>`.
- Loads `elm.js`, generated from `src/Main.elm`.
- Loads `web/tauri.js` as a module.
- Links `web/styles/app.css`.

There is no frontend bundler. The build script compiles Elm directly into
`web/elm.js`, and Tauri serves the `web/` directory.

## JavaScript Bridge

`web/tauri.js` has one responsibility: translate Elm port messages into Tauri
command invocations.

Elm sends:

```json
{
  "requestId": 1,
  "command": "select_message",
  "payload": {
    "messageId": "m-1001"
  }
}
```

The bridge calls:

```js
window.__TAURI__.core.invoke(command, payload)
```

Then it sends Elm a normalized response:

```json
{
  "requestId": 1,
  "command": "select_message",
  "ok": true,
  "data": {},
  "error": null
}
```

The `requestId` lets Elm match async responses to the original request type.
That keeps command response handling explicit even when several requests are in
flight.

## Elm App Structure

The frontend uses The Elm Architecture:

```text
Model  -> current state
Msg    -> user events and backend responses
update -> state transitions and commands
view   -> HTML from Model
```

`src/Main.elm` is a `port module` because Tauri interop happens through ports:

```elm
port commandOut : Encode.Value -> Cmd msg
port commandIn : (Encode.Value -> msg) -> Sub msg
```

### Model

The `Model` stores all UI state:

- `accounts`
- `folders`
- `messages`
- `selectedFolderId`
- `selectedMessageId`
- `selectedMessage`
- `syncStatus`
- `search`
- `pending`
- `nextRequestId`
- compose modal fields
- loading and error state

The backend may return new mailbox snapshots, but the frontend decides how they
are represented on screen.

### Messages

Important `Msg` values:

- `SelectFolder String`
- `SelectMessage String`
- `SearchInput String`
- `RunSearch`
- `Refresh`
- `ToggleSelectedRead`
- `ArchiveSelected`
- compose field messages
- `GotCommand Encode.Value`

User-facing events are concrete. Backend responses are funneled through
`GotCommand` and decoded according to the request that generated them.

### Command Queue

Backend calls go through `enqueue`.

`enqueue` does three things:

1. Allocates a request ID.
2. Stores `requestId -> RequestKind` in `pending`.
3. Sends `{ requestId, command, payload }` through `commandOut`.

`RequestKind` is separate from the command string so Elm can decode each
response into the right shape:

- `BootstrapRequest` decodes `BootstrapData`.
- `SelectFolderRequest`, `SearchRequest`, and `ArchiveRequest` decode
  `MailboxSnapshot`.
- `SelectMessageRequest` and `MarkReadRequest` decode `MessageUpdate`.

## Backend Response Shapes

The frontend expects three main response shapes.

### BootstrapData

Used on app startup and refresh:

```text
accounts
folders
messages
selectedFolderId
selectedMessageId
selectedMessage
syncStatus
```

### MailboxSnapshot

Used when the visible mailbox changes:

```text
folderId
folders
messages
selectedMessageId
selectedMessage
syncStatus
```

### MessageUpdate

Used when a single selected message changes:

```text
folders
message
syncStatus
```

`folders` is included in message responses so unread badges stay accurate after
read/unread changes.

## View Layout

The top-level `view` renders three persistent panes plus an optional composer:

```text
view
├── viewSidebar
├── viewListPane
├── viewDetailPane
└── viewComposer, when composeOpen is true
```

### Sidebar

`viewSidebar` renders:

- app title
- refresh and compose buttons
- accounts
- folders
- unread counts
- sync status

### Message List Pane

`viewListPane` renders:

- search box
- current folder title
- message count and unread count
- message rows

Each row sends `SelectMessage message.id`.

### Detail Pane

`viewDetailPane` renders:

- command status or error
- read/unread action
- archive action
- compose action
- selected message body

### Composer

`viewComposer` is local UI state only right now. `SendCompose` clears fields and
updates local sync status. It does not call Rust yet.

## Styling

All styling lives in `web/styles/app.css`.

The layout is a fixed desktop mail client shell:

```text
244px sidebar
minmax(360px, 440px) message list
minmax(380px, 1fr) detail pane
```

The CSS uses stable dimensions for panes, buttons, rows, and composer controls
so message content does not resize the application layout.

## Current Limitations

This is still a scaffold:

- No real IMAP, JMAP, Gmail, or Outlook provider integration.
- No persisted local message store.
- No credential or keychain handling.
- Composer does not send mail.
- Search runs against the mock in-memory Rust store.

Those pieces can be added behind the existing Tauri command boundary without
rewriting the Elm view architecture.

## Extension Points

Recommended next additions:

1. Add a Rust storage layer, probably SQLite, below the current command API.
2. Add account setup commands and keep OAuth/keychain work on the Rust side.
3. Replace the in-memory `MailStore` with provider-backed sync.
4. Add command responses for draft creation and send progress.
5. Add focused Elm tests around response decoders before the backend contract
   grows.
