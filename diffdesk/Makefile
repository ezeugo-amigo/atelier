SHELL := /bin/bash

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
BUN ?= bun
ROOT := $(CURDIR)
VITE_URL ?= http://127.0.0.1:1420

# Directory holding the active node binary (and bun). nvm-managed node is not on
# PATH in non-login shells, so we resolve it here and (a) use it for the build and
# (b) bake it into the generated wrapper so it can start Vite anywhere.
# Resolved at install time; re-run `make install` after upgrading node.
NODE_BIN := $(shell command -v node >/dev/null 2>&1 && dirname "$$(command -v node)")
ifneq ($(NODE_BIN),)
export PATH := $(NODE_BIN):$(PATH)
endif

# Absolute path to bun, resolved at install time and baked into the wrapper so it
# works from shells where bun is only a shell alias (or absent from PATH).
BUN_BIN := $(shell command -v $(BUN) 2>/dev/null)

.PHONY: build install uninstall which

build:
	@if ! command -v $(BUN) >/dev/null 2>&1; then \
		echo "diffdesk: bun not found on PATH; install it (e.g. brew install bun)" >&2; \
		exit 1; \
	fi
	@if [ ! -d node_modules ]; then \
		echo "diffdesk: installing frontend dependencies"; \
		$(BUN) install; \
	fi
	cargo build -p diffdesk-cli --release
	cargo build -p diffdesk-app

install: build
	@mkdir -p "$(BINDIR)"
	@{ \
		printf '%s\n' '#!/usr/bin/env bash'; \
		printf '%s\n' 'set -euo pipefail'; \
		printf '%s\n' ''; \
		printf '%s\n' 'ROOT="$(ROOT)"'; \
		printf '%s\n' 'VITE_URL="$(VITE_URL)"'; \
		printf '%s\n' 'CLI="$$ROOT/target/release/diffdesk"'; \
		printf '%s\n' 'APP="$$ROOT/target/debug/diffdesk-app"'; \
		printf '%s\n' 'LOG_DIR="$$HOME/.diffdesk/logs"'; \
		printf '%s\n' 'VITE_LOG="$$LOG_DIR/vite.log"'; \
		printf '%s\n' ''; \
		printf '%s\n' 'mkdir -p "$$LOG_DIR"'; \
		printf '%s\n' '# nvm-managed node/bun are absent from non-login (e.g. agent) shells;'; \
		printf '%s\n' '# put the install-time node bin dir on PATH so Vite can be started anywhere.'; \
		printf '%s\n' 'NODE_BIN="$(NODE_BIN)"'; \
		printf '%s\n' 'if [ -n "$$NODE_BIN" ] && [ -d "$$NODE_BIN" ]; then PATH="$$NODE_BIN:$$PATH"; fi'; \
		printf '%s\n' 'BUN="$(BUN_BIN)"'; \
		printf '%s\n' 'if [ ! -x "$$BUN" ]; then BUN="$$(command -v bun || true)"; fi'; \
		printf '%s\n' 'require_bun() {'; \
		printf '%s\n' '  if [ -z "$$BUN" ] || [ ! -x "$$BUN" ]; then'; \
		printf '%s\n' '    echo "diffdesk: bun not found; install it (e.g. brew install bun) and re-run make install" >&2'; \
		printf '%s\n' '    exit 2'; \
		printf '%s\n' '  fi'; \
		printf '%s\n' '}'; \
		printf '%s\n' ''; \
		printf '%s\n' 'if [ ! -x "$$CLI" ] || [ ! -x "$$APP" ]; then'; \
		printf '%s\n' '  echo "diffdesk: building missing binaries in $$ROOT..." >&2'; \
		printf '%s\n' '  if [ ! -d "$$ROOT/node_modules" ]; then'; \
		printf '%s\n' '    require_bun'; \
		printf '%s\n' '    (cd "$$ROOT" && "$$BUN" install)'; \
		printf '%s\n' '  fi'; \
		printf '%s\n' '  (cd "$$ROOT" && cargo build -p diffdesk-cli --release && cargo build -p diffdesk-app)'; \
		printf '%s\n' 'fi'; \
		printf '%s\n' ''; \
		printf '%s\n' 'if ! curl -fsS "$$VITE_URL" >/dev/null 2>&1; then'; \
		printf '%s\n' '  existing_pids="$$(lsof -tiTCP:1420 -sTCP:LISTEN 2>/dev/null || true)"'; \
		printf '%s\n' '  if [ -n "$$existing_pids" ]; then'; \
		printf '%s\n' '    echo "diffdesk: stopping stale server on port 1420: $$existing_pids" >&2'; \
		printf '%s\n' '    kill $$existing_pids 2>/dev/null || true'; \
		printf '%s\n' '    sleep 0.3'; \
		printf '%s\n' '  fi'; \
		printf '%s\n' '  require_bun'; \
		printf '%s\n' '  echo "diffdesk: starting Vite dev server at $$VITE_URL..." >&2'; \
		printf '%s\n' '  (cd "$$ROOT" && nohup "$$BUN" vite >"$$VITE_LOG" 2>&1 & echo $$! > "$$LOG_DIR/vite.pid")'; \
		printf '%s\n' '  for _ in {1..50}; do'; \
		printf '%s\n' '    curl -fsS "$$VITE_URL" >/dev/null 2>&1 && break'; \
		printf '%s\n' '    sleep 0.1'; \
		printf '%s\n' '  done'; \
		printf '%s\n' 'fi'; \
		printf '%s\n' ''; \
		printf '%s\n' 'if ! curl -fsS "$$VITE_URL" >/dev/null 2>&1; then'; \
		printf '%s\n' '  echo "diffdesk: Vite did not start; see $$VITE_LOG" >&2'; \
		printf '%s\n' '  exit 2'; \
		printf '%s\n' 'fi'; \
		printf '%s\n' ''; \
		printf '%s\n' 'export DIFFDESK_APP="$$APP"'; \
		printf '%s\n' 'exec "$$CLI" "$$@"'; \
	} > "$(BINDIR)/diffdesk"
	@chmod +x "$(BINDIR)/diffdesk"
	@echo "Installed diffdesk -> $(BINDIR)/diffdesk"
	@echo "Pinned to $(ROOT)"
	@echo "Uses debug app + auto-started Vite server at $(VITE_URL)"

uninstall:
	@rm -f "$(BINDIR)/diffdesk"
	@echo "Removed $(BINDIR)/diffdesk"

which:
	@command -v diffdesk || true
	@if [ -x "$(BINDIR)/diffdesk" ]; then \
		echo "$(BINDIR)/diffdesk points to:"; \
		grep '^ROOT=' "$(BINDIR)/diffdesk"; \
	fi
