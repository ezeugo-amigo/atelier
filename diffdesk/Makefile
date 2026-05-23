SHELL := /bin/bash

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
PNPM ?= corepack pnpm
ROOT := $(CURDIR)

.PHONY: build install uninstall which

build:
	@if [ ! -d node_modules ]; then \
		echo "diffdesk: installing frontend dependencies"; \
		$(PNPM) install; \
	fi
	$(PNPM) vite:build
	cargo build -p diffdesk-cli -p diffdesk-app --release

install: build
	@mkdir -p "$(BINDIR)"
	@{ \
		printf '%s\n' '#!/usr/bin/env bash'; \
		printf '%s\n' 'set -euo pipefail'; \
		printf '%s\n' ''; \
		printf '%s\n' 'ROOT="$(ROOT)"'; \
		printf '%s\n' 'CLI="$$ROOT/target/release/diffdesk"'; \
		printf '%s\n' 'APP="$$ROOT/target/release/diffdesk-app"'; \
		printf '%s\n' ''; \
		printf '%s\n' 'if [ ! -x "$$CLI" ] || [ ! -x "$$APP" ]; then'; \
		printf '%s\n' '  echo "diffdesk: building release binaries in $$ROOT..." >&2'; \
		printf '%s\n' '  if [ ! -d "$$ROOT/node_modules" ]; then'; \
		printf '%s\n' '    (cd "$$ROOT" && corepack pnpm install)'; \
		printf '%s\n' '  fi'; \
		printf '%s\n' '  (cd "$$ROOT" && corepack pnpm vite:build && cargo build -p diffdesk-cli -p diffdesk-app --release)'; \
		printf '%s\n' 'fi'; \
		printf '%s\n' ''; \
		printf '%s\n' 'export DIFFDESK_APP="$$APP"'; \
		printf '%s\n' 'exec "$$CLI" "$$@"'; \
	} > "$(BINDIR)/diffdesk"
	@chmod +x "$(BINDIR)/diffdesk"
	@echo "Installed diffdesk -> $(BINDIR)/diffdesk"
	@echo "Pinned to $(ROOT)"

uninstall:
	@rm -f "$(BINDIR)/diffdesk"
	@echo "Removed $(BINDIR)/diffdesk"

which:
	@command -v diffdesk || true
	@if [ -x "$(BINDIR)/diffdesk" ]; then \
		echo "$(BINDIR)/diffdesk points to:"; \
		grep '^ROOT=' "$(BINDIR)/diffdesk"; \
	fi
