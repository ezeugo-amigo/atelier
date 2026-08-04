# Diffdesk

A terminal-launched Tauri desktop diff reviewer with inline comments and AI-ready export.

## Current MVP

- Open a Git working-tree diff, staged diff, commit/range diff, patch file, or stdin diff.
- Review changes in a polished dark-mode desktop UI.
- Add inline line comments with severity.
- Add a global summary.
- Autosave drafts under `~/.diffdesk/sessions/<session-id>/drafts.json`.
- Remember which files have been viewed, and only reopen files whose diff content changed.
- Submit review as Markdown or JSON.
- CLI wait mode watches for `result.json` and returns control to the terminal.

## Development

```bash
pnpm install
pnpm dev
```

To open a specific diff through the Tauri app in development:

```bash
pnpm dev -- --staged
pnpm dev -- main...HEAD
pnpm dev -- changes.patch
```

## CLI workflow

Build the app binary first:

```bash
pnpm build
```

Then build/run the CLI:

```bash
cargo run -p diffdesk-cli -- --staged --app-command ./src-tauri/target/debug/diffdesk-app
```

During development you can also set:

```bash
export DIFFDESK_APP=/Users/ezeugo/diffdesk/src-tauri/target/debug/diffdesk-app
cargo run -p diffdesk-cli -- --staged --output /tmp/review.md
```

Final packaged command behavior is intended to be:

```bash
diffdesk
diffdesk --staged
diffdesk main...HEAD
git diff | diffdesk -
diffdesk --wait --output review.md
```

## Session files

Diffdesk stores local review sessions at:

```text
~/.diffdesk/sessions/<session-id>/
  session.json
  input.diff
  drafts.json
  result.json
```

Viewed-file state is stored at `~/.diffdesk/review-state.json` and is keyed by diff
source, file path, and file content. Marking a file as viewed records the current
version immediately; Diffdesk also flushes the state file as part of window-close,
submit, and cancel cleanup. When that file changes in a later review, it is shown
as unreviewed again.

## Output formats

Markdown output is optimized for AI handoff: it includes source metadata, human summary, inline comments, severity, file paths, line numbers, and relevant line context.

JSON output preserves structured comments and anchors for tool integrations.
