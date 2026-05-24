SHELL := /bin/bash

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
PNPM ?= corepack pnpm
ROOT := $(CURDIR)
VITE_URL ?= http://127.0.0.1:1420

.PHONY: build install uninstall which

build:
	@corepack enable
	@if [ ! -d node_modules ]; then \
		echo "diffdesk: installing frontend dependencies"; \
		$(PNPM) install; \
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
		printf '%s\n' 'corepack enable >/dev/null 2>&1 || true'; \
		printf '%s\n' ''; \
		printf '%s\n' 'if [ ! -x "$$CLI" ] || [ ! -x "$$APP" ]; then'; \
		printf '%s\n' '  echo "diffdesk: building missing binaries in $$ROOT..." >&2'; \
		printf '%s\n' '  if [ ! -d "$$ROOT/node_modules" ]; then'; \
		printf '%s\n' '    (cd "$$ROOT" && corepack pnpm install)'; \
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
		printf '%s\n' '  echo "diffdesk: starting Vite dev server at $$VITE_URL..." >&2'; \
		printf '%s\n' '  (cd "$$ROOT" && nohup corepack pnpm vite >"$$VITE_LOG" 2>&1 & echo $$! > "$$LOG_DIR/vite.pid")'; \
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
