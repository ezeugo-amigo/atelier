SHELL := /bin/bash

# Ghost — build / install / launch (macOS).
#
# A Tauri bundle with the TypeScript frontend embedded. `make install` builds a real .app
# and copies it into /Applications; `make dev` runs Tauri's dev mode with hot reload.

APP_NAME    := Ghost
BUNDLE_DIR  := src-tauri/target/release/bundle/macos
APP_BUNDLE  := $(BUNDLE_DIR)/$(APP_NAME).app
INSTALL_DIR := /Applications
INSTALLED   := $(INSTALL_DIR)/$(APP_NAME).app

TAURI_CLI_VERSION := ^2.0

.DEFAULT_GOAL := help

.PHONY: help deps tauri-cli build install launch dev test uninstall clean

help:
	@echo "Ghost — make targets:"
	@echo "  make install    build a release .app and copy it into $(INSTALL_DIR)"
	@echo "  make launch     open the installed app (installs it first if needed)"
	@echo "  make dev        run Tauri dev mode (frontend hot-reloads)"
	@echo "  make build      build the release .app bundle without installing"
	@echo "  make test       typecheck and run the unit tests"
	@echo "  make uninstall  remove $(APP_NAME).app from $(INSTALL_DIR)"
	@echo "  make clean      remove build artifacts"

deps:
	@bun install

tauri-cli:
	@if ! cargo tauri --version >/dev/null 2>&1; then \
		echo "Ghost: installing tauri-cli ($(TAURI_CLI_VERSION)) — this can take a few minutes..."; \
		cargo install tauri-cli --version "$(TAURI_CLI_VERSION)" --locked; \
	fi

build: deps tauri-cli
	cargo tauri build

install: build
	@rm -rf "$(INSTALLED)"
	@cp -R "$(APP_BUNDLE)" "$(INSTALL_DIR)/"
	@echo "Installed: $(INSTALLED)"

launch:
	@if [ ! -d "$(INSTALLED)" ]; then $(MAKE) install; fi
	@open "$(INSTALLED)"

dev: deps tauri-cli
	cargo tauri dev

test: deps
	bunx tsc --noEmit
	bun test

uninstall:
	@rm -rf "$(INSTALLED)"
	@echo "Removed $(INSTALLED)"

clean:
	@rm -rf src-tauri/target dist
	@echo "Cleaned build artifacts"
