SHELL := /bin/bash

.DEFAULT_GOAL := help

.PHONY: help build test install uninstall reinstall which clean

help:
	@echo "devhost — make targets:"
	@echo "  make install    build and install devhost into Cargo's bin dir"
	@echo "  make uninstall  remove the installed devhost binary"
	@echo "  make reinstall  uninstall, then install"
	@echo "  make build      build the debug binary"
	@echo "  make test       run tests"
	@echo "  make which      show the active devhost on PATH"
	@echo "  make clean      remove build artifacts"
	@echo ""
	@echo "Install location: $$(rustup which cargo >/dev/null 2>&1 && cargo install --list >/dev/null 2>&1 && echo '$$HOME/.cargo/bin/devhost' || echo 'Cargo bin dir')"
	@echo "Make sure $$HOME/.cargo/bin is on PATH to run 'devhost' anywhere."

build:
	cargo build

test:
	cargo test

install:
	cargo install --path . --locked --force
	@echo "Installed devhost -> $$(command -v devhost || echo '$$HOME/.cargo/bin/devhost')"
	@echo "Try: devhost --help"

uninstall:
	cargo uninstall devhost || true

reinstall: uninstall install

which:
	@command -v devhost || { echo "devhost is not on PATH"; exit 1; }
	@devhost --version 2>/dev/null || devhost --help | head -1

clean:
	cargo clean
