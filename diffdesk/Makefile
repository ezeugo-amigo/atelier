SHELL := /bin/bash

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
PNPM ?= corepack pnpm
ROOT := $(CURDIR)

.PHONY: build install uninstall which

build:
	@corepack enable
	@if [ ! -d node_modules ]; then \
		echo "diffdesk: installing frontend dependencies"; \
		$(PNPM) install; \
	fi
	cargo build -p diffdesk-cli --release
	$(PNPM) tauri build --bundles app

install: build
	@mkdir -p "$(BINDIR)"
	@{ \
		printf '%s\n' '#!/usr/bin/env bash'; \
		printf '%s\n' 'set -euo pipefail'; \
		printf '%s\n' ''; \
		printf '%s\n' 'ROOT="$(ROOT)"'; \
		printf '%s\n' 'CLI="$$ROOT/target/release/diffdesk"'; \
		printf '%s\n' 'APP_BUNDLE="$$ROOT/target/release/bundle/macos/Diffdesk.app"'; \
		printf '%s\n' 'APP_EXECUTABLE="$$ROOT/target/release/bundle/macos/Diffdesk.app/Contents/MacOS/diffdesk-app"'; \
		printf '%s\n' 'APP_BINARY="$$ROOT/target/release/diffdesk-app"'; \
		printf '%s\n' ''; \
		printf '%s\n' 'if [ ! -x "$$CLI" ] || [ ! -d "$$APP_BUNDLE" ]; then'; \
		printf '%s\n' '  echo "diffdesk: building release app in $$ROOT..." >&2'; \
		printf '%s\n' '  corepack enable'; \
		printf '%s\n' '  if [ ! -d "$$ROOT/node_modules" ]; then'; \
		printf '%s\n' '    (cd "$$ROOT" && corepack pnpm install)'; \
		printf '%s\n' '  fi'; \
		printf '%s\n' '  (cd "$$ROOT" && cargo build -p diffdesk-cli --release && corepack pnpm tauri build --bundles app)'; \
		printf '%s\n' 'fi'; \
		printf '%s\n' ''; \
		printf '%s\n' 'if [ -x "$$APP_EXECUTABLE" ]; then'; \
		printf '%s\n' '  export DIFFDESK_APP="$$APP_EXECUTABLE"'; \
		printf '%s\n' 'else'; \
		printf '%s\n' '  export DIFFDESK_APP="$$APP_BINARY"'; \
		printf '%s\n' 'fi'; \
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
