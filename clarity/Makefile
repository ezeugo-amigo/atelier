SHELL := /bin/bash

# Clarity — build / install / launch (macOS).
#
# Clarity is a Swift package, so it builds with just the Command Line Tools (no Xcode project).
# `make build` compiles a release binary and assembles a real .app around it: Info.plist, the
# bundled iA Writer Mono fonts, and an ad-hoc signature.

APP_NAME    := Clarity
APP_BUNDLE  := build/$(APP_NAME).app
INSTALL_DIR := /Applications
INSTALLED   := $(INSTALL_DIR)/$(APP_NAME).app

.DEFAULT_GOAL := help

.PHONY: help build install launch run icon uninstall clean

help:
	@echo "Clarity — make targets:"
	@echo "  make install    build the .app and copy it into $(INSTALL_DIR)"
	@echo "  make launch     open the installed app (installs it first if needed)"
	@echo "  make run        build + run the debug binary directly (no bundle/install)"
	@echo "  make build      build $(APP_BUNDLE) without installing"
	@echo "  make icon       re-render Resources/AppIcon.icns from scripts/make-icon.swift"
	@echo "  make uninstall  remove $(APP_NAME).app from $(INSTALL_DIR)"
	@echo "  make clean      remove build artifacts"

build:
	swift build -c release
	@rm -rf "$(APP_BUNDLE)"
	@mkdir -p "$(APP_BUNDLE)/Contents/MacOS" "$(APP_BUNDLE)/Contents/Resources"
	@cp "$$(swift build -c release --show-bin-path)/$(APP_NAME)" "$(APP_BUNDLE)/Contents/MacOS/"
	@cp Resources/Info.plist "$(APP_BUNDLE)/Contents/"
	@cp -R Resources/Fonts Resources/AppIcon.icns "$(APP_BUNDLE)/Contents/Resources/"
	@codesign --force --sign - "$(APP_BUNDLE)"
	@echo "Built: $(APP_BUNDLE)"

install: build
	@echo "Clarity: installing $(APP_NAME).app -> $(INSTALL_DIR)"
	@rm -rf "$(INSTALLED)"
	@cp -R "$(APP_BUNDLE)" "$(INSTALL_DIR)/"
	@echo "Installed: $(INSTALLED)"

launch:
	@if [ ! -d "$(INSTALLED)" ]; then $(MAKE) install; fi
	@open "$(INSTALLED)"

# Pass a folder or file to open: make run ARGS=~/notes
run:
	swift run $(APP_NAME) $(ARGS)

icon:
	swift scripts/make-icon.swift

uninstall:
	@rm -rf "$(INSTALLED)"
	@echo "Removed $(INSTALLED)"

clean:
	@rm -rf .build build
	@echo "Cleaned build artifacts"
