# devhost

Local reverse-proxy runner for many development app instances.

`devhost` gives each running app instance a stable `.localhost` hostname derived
from the app name, repository, and branch/worktree context.

```sh
devhost proxy
devhost run web -- pnpm dev
# => http://web-atelier-port-handling.localhost:8080
```

## Install

```sh
cd devhost
make install
```

This installs the `devhost` binary with `cargo install --path . --locked --force`,
usually to:

```text
~/.cargo/bin/devhost
```

Make sure `~/.cargo/bin` is on your `PATH`, then `devhost` works from any
repository or worktree.

## Commands

```sh
devhost proxy [--bind 127.0.0.1] [--port 8080]
devhost run <app> [--host <host-prefix>] [--port <port>] -- <command...>
devhost ps
devhost clean
```

The first version is intentionally foreground-first: `run` starts the child,
streams its output, registers the route, and removes the route when the child
exits or you press Ctrl-C.

## Hostnames

Default hostname format:

```text
<app>-<repo>-<context>.localhost
```

- `repo` comes from `remote.origin.url` when possible, then falls back to the
  git root directory name.
- `context` is the current branch name, detached short SHA, or current directory
  name outside git.
- `--host` overrides the generated host prefix exactly.

## State

Routes live in:

```text
~/.config/devhost/state.json
```

Set `DEVHOST_STATE=/path/to/state.json` to use a different state file during
experiments or tests.

The proxy reloads this file for each connection, so `run` commands can add and
remove routes without restarting the proxy.
