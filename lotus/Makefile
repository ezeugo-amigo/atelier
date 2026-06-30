SHELL := /bin/bash

APP_NAME    := Lotus
BUNDLE_DIR  := src-tauri/target/release/bundle/macos
APP_BUNDLE  := $(BUNDLE_DIR)/$(APP_NAME).app
INSTALL_DIR := /Applications
INSTALLED   := $(INSTALL_DIR)/$(APP_NAME).app
DEBUG_BIN   := src-tauri/target/debug/lotus-app

TAURI_CLI_VERSION := ^2.0

.DEFAULT_GOAL := help

.PHONY: help web tauri-cli build install launch run dev clean

help:
	@echo "Lotus — make targets:"
	@echo "  make dev        run Tauri dev mode"
	@echo "  make run        build + run the debug binary directly"
	@echo "  make build      build the release .app bundle"
	@echo "  make web        compile just the Elm frontend -> web/elm.js"
	@echo "  make install    build and copy $(APP_NAME).app into $(INSTALL_DIR)"
	@echo "  make launch     open the installed app"
	@echo "  make clean      remove build artifacts"

web:
	@bash build.sh --optimize

tauri-cli:
	@if ! cargo tauri --version >/dev/null 2>&1; then \
		echo "Lotus: installing tauri-cli ($(TAURI_CLI_VERSION))"; \
		cargo install tauri-cli --version "$(TAURI_CLI_VERSION)" --locked; \
	fi

build: tauri-cli
	cargo tauri build

install: build
	@echo "Lotus: installing $(APP_NAME).app -> $(INSTALL_DIR)"
	@rm -rf "$(INSTALLED)"
	@cp -R "$(APP_BUNDLE)" "$(INSTALL_DIR)/"
	@echo "Installed: $(INSTALLED)"

launch:
	@if [ ! -d "$(INSTALLED)" ]; then $(MAKE) install; fi
	@open "$(INSTALLED)"

run: web
	cd src-tauri && cargo build
	@./$(DEBUG_BIN)

dev: tauri-cli
	cargo tauri dev

clean:
	@rm -rf src-tauri/target web/elm.js
	@echo "Cleaned build artifacts"
