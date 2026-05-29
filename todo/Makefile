SHELL := /bin/bash

# Today — build / install / launch (macOS).
#
# The app is a self-contained Tauri bundle with the Elm frontend embedded, so
# "install" means building a real .app and copying it into /Applications, and
# "launch" means opening it. For quick iteration, `make run` skips bundling and
# runs the debug binary directly, and `make dev` runs Tauri's dev mode.

APP_NAME    := Today
BUNDLE_DIR  := src-tauri/target/release/bundle/macos
APP_BUNDLE  := $(BUNDLE_DIR)/$(APP_NAME).app
INSTALL_DIR := /Applications
INSTALLED   := $(INSTALL_DIR)/$(APP_NAME).app
DEBUG_BIN   := src-tauri/target/debug/today-app

# Tauri CLI v2, used as the `cargo tauri ...` subcommand.
TAURI_CLI_VERSION := ^2.0

.DEFAULT_GOAL := help

.PHONY: help web tauri-cli build install launch run dev uninstall clean

help:
	@echo "Today — make targets:"
	@echo "  make install    build a release .app and copy it into $(INSTALL_DIR)"
	@echo "  make launch     open the installed app (installs it first if needed)"
	@echo "  make run        build + run the debug binary directly (no bundle/install)"
	@echo "  make dev        run Tauri dev mode (rebuilds the frontend on launch)"
	@echo "  make build      build the release .app bundle without installing"
	@echo "  make web        compile just the Elm frontend -> web/elm.js"
	@echo "  make uninstall  remove $(APP_NAME).app from $(INSTALL_DIR)"
	@echo "  make clean      remove build artifacts"

# Compile the Elm frontend that the shell bundles/serves.
web:
	@bash build.sh --optimize

# Ensure the Tauri CLI (cargo subcommand) is available; install it if missing.
tauri-cli:
	@if ! cargo tauri --version >/dev/null 2>&1; then \
		echo "Today: installing tauri-cli ($(TAURI_CLI_VERSION)) — this can take a few minutes..."; \
		cargo install tauri-cli --version "$(TAURI_CLI_VERSION)" --locked; \
	fi

# Build a release .app bundle (+ dmg). tauri.conf.json's beforeBuildCommand
# compiles the Elm frontend (./build.sh --optimize) before cargo runs.
build: tauri-cli
	cargo tauri build

# Build the release app and copy it into /Applications.
install: build
	@echo "Today: installing $(APP_NAME).app -> $(INSTALL_DIR)"
	@rm -rf "$(INSTALLED)"
	@cp -R "$(APP_BUNDLE)" "$(INSTALL_DIR)/"
	@echo "Installed: $(INSTALLED)"

# Open the installed app, building + installing it first if it isn't there yet.
launch:
	@if [ ! -d "$(INSTALLED)" ]; then $(MAKE) install; fi
	@echo "Today: launching $(APP_NAME)"
	@open "$(INSTALLED)"

# Quick local run: debug binary with the embedded frontend. No CLI, no bundle.
run: web
	cd src-tauri && cargo build
	@echo "Today: launching debug build"
	@./$(DEBUG_BIN)

# Development mode — opens the app and rebuilds the frontend via build.sh.
dev: tauri-cli
	cargo tauri dev

uninstall:
	@rm -rf "$(INSTALLED)"
	@echo "Removed $(INSTALLED)"

clean:
	@rm -rf src-tauri/target web/elm.js
	@echo "Cleaned build artifacts"
