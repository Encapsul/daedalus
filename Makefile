HOST_ARCH ?= $(shell uname -m)
HOST_OS  ?= linux
TARGET   ?= $(HOST_ARCH)-unknown-linux-musl
TOOLS    := /tmp/xbin-stub-target
VERSION  ?= $(shell cargo pkgid -p xbin-core 2>/dev/null | awk -F'#' '{print $$2}' || echo "0.4.0")
DIST_DIR := dist
# Cargo target dir — override to keep builds off a full root fs, e.g.
#   make dist CARGO_TARGET_DIR=/tmp/xbin-release-target
export CARGO_TARGET_DIR := $(or $(CARGO_TARGET_DIR),$(CURDIR)/target)

STUB    := $(TOOLS)/$(TARGET)/release/xbin-stub
CRYPTO  := $(TOOLS)/$(TARGET)/release/xbin-crypto
CLI     := target/release/xbin
STUB_ALT := target/$(TARGET)/release/xbin-stub
CRYPTO_ALT := target/$(TARGET)/release/xbin-crypto

# Architectures to build for in `dist` target
ARCHS := x86_64 aarch64

.PHONY: all preflight stub cli install example run inspect docs docs-serve lint fmt clean dist release help

all: stub cli

# Verify all prerequisites are installed before building.
preflight:
	@command -v cargo >/dev/null 2>&1 || { echo "FAIL: cargo not found (install Rust: https://rustup.rs)"; exit 1; }
	@command -v rustc >/dev/null 2>&1 || { echo "FAIL: rustc not found (install Rust: https://rustup.rs)"; exit 1; }
	@rustup target list --installed 2>/dev/null | grep -q "$(HOST_ARCH)-unknown-linux-musl" || { echo "FAIL: musl target missing (run: rustup target add $(HOST_ARCH)-unknown-linux-musl)"; exit 1; }
	@command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || { echo "FAIL: C compiler not found (install gcc or musl-tools)"; exit 1; }
	@command -v zstd >/dev/null 2>&1 || { echo "FAIL: zstd not found (install: apt install zstd)"; exit 1; }
	@echo "all prerequisites OK"

# Build the Rust stub (statically linked musl ELF).
stub:
	cargo build --release -p xbin-stub --target $(TARGET)
	@if [ -f $(STUB) ]; then \
		echo "stub:   $$(ls -la $(STUB) | awk '{print $$5}') bytes"; \
	elif [ -f $(STUB_ALT) ]; then \
		echo "stub:   $$(ls -la $(STUB_ALT) | awk '{print $$5}') bytes"; \
	else \
		echo "stub:   NOT FOUND"; \
	fi
	@if [ -f $(CRYPTO) ]; then \
		echo "crypto: $$(ls -la $(CRYPTO) | awk '{print $$5}') bytes"; \
	elif [ -f $(CRYPTO_ALT) ]; then \
		echo "crypto: $$(ls -la $(CRYPTO_ALT) | awk '{print $$5}') bytes"; \
	else \
		echo "crypto:   NOT FOUND"; \
	fi

# Build the Rust CLI.
cli:
	cargo build --release -p xbin-cli
	@if [ -f $(CLI) ]; then \
		echo "cli:    $$(ls -la $(CLI) | awk '{print $$5}') bytes"; \
	else \
		echo "cli:    NOT FOUND"; \
	fi

# Hybrid install: tries system-wide, falls back to user-level
install:
	@printf "\nBuilding x.bin (this may take a minute)...\n"
	cargo build --release -p xbin-cli
	@if sudo test -w /usr/local/bin 2>/dev/null; then \
		printf "\nInstalling to /usr/local/bin...\n"; \
		sudo cp target/release/xbin /usr/local/bin/xbin; \
		sudo chmod +x /usr/local/bin/xbin; \
		printf "\n✅ Installed to /usr/local/bin/xbin\n"; \
		printf "   Verify: xbin --version\n"; \
		printf "   You can now remove the x.bin repo!\n"; \
	else \
		printf "\nInstalling to ~/.local/bin (no admin required)...\n"; \
		mkdir -p ~/.local/bin; \
		cp target/release/xbin ~/.local/bin/xbin; \
		chmod +x ~/.local/bin/xbin; \
		printf "\n✅ Installed to ~/.local/bin/xbin\n"; \
		printf "   Add ~/.local/bin to your PATH:\n"; \
		printf "   echo 'export PATH=\"\$$HOME/.local/bin:\$$PATH\"' >> \$$HOME/.bashrc\n"; \
		printf "   source \$$HOME/.bashrc\n"; \
		printf "   Then verify: xbin --version\n"; \
		printf "   You can remove the x.bin repo when done.\n"; \
	fi

# Build the hello-web example .xbin (requires stub + cli).
example: stub cli
	$(CLI) build examples/hello-web -o hello-web.xbin

# Run the hello-web .xbin.
run:
	./hello-web.xbin

# Inspect the hello-web .xbin.
inspect:
	$(CLI) inspect hello-web.xbin

# Build mdbook documentation.
docs:
	cd docs && mdbook build

docs-serve:
	cd docs && mdbook serve --open

lint:
	cargo clippy -p xbin-core --all-targets -- -D warnings
	cargo clippy -p xbin-stub -- -D warnings
	cargo clippy -p xbin-cli -- -D warnings

fmt:
	cargo fmt --all

clean:
	cargo clean
	cd stub && cargo clean
	rm -rf docs/book
	rm -f *.xbin
	rm -rf ~/.cache/xbin $(DIST_DIR)

# ═══════════════════════════════════════════════════════════════════
# Release / Distribution Targets
# ═══════════════════════════════════════════════════════════════════
#
# Naming convention for release assets (glow-style):
#   xbin_<version>_<os>_<arch>.<ext>
#
# Each Linux archive bundles the three binaries produced by this
# workspace for that platform:
#   xbin          — the CLI tool
#   xbin-stub     — the statically linked launcher (musl ELF)
#   xbin-crypto   — the signing/inspection utility (musl ELF)
#
# Components are Linux-only today (the stub launcher is Linux-only and
# xbin-core uses std::os::unix APIs), so this release ships Linux
# amd64 and arm64. macOS and Windows can be added once the workspace
# builds without those Unix dependencies and a macOS SDK is available.
#
# Examples:
#   xbin_0.4.0_linux_amd64.tar.gz   (xbin + xbin-stub + xbin-crypto)
#   xbin_0.4.0_linux_arm64.tar.gz
#   checksums.txt
#
# `dist` uses cargo-zigbuild so the musl artifacts are produced via the
# Zig C compiler (zig must be on PATH; no mingw/musl-gcc required):

ZIGBUILD := cargo zigbuild

# Build all release artifacts for all architectures.
# Output: $(DIST_DIR)/ with one tarball per arch + checksums.txt
dist: preflight
	@mkdir -p $(DIST_DIR)
	@command -v zig >/dev/null 2>&1 || { echo "FAIL: zig not found (install https://ziglang.org/download)"; exit 1; }
	@command -v sha256sum >/dev/null 2>&1 || { echo "FAIL: sha256sum not found"; exit 1; }
	@echo "Building release v$(VERSION) for $(ARCHS)..."
	@for arch in $(ARCHS); do \
		target="$$arch-unknown-linux-musl"; \
		printf "\n── $$arch (musl, static) ──\n"; \
		$(ZIGBUILD) --release -p xbin-stub -p xbin-crypto --target "$$target"; \
		$(ZIGBUILD) --release -p xbin-cli --target "$$target"; \
		dir="xbin_$(VERSION)_linux_$$arch"; \
		rm -rf "$(DIST_DIR)/$$dir"; \
		mkdir -p "$(DIST_DIR)/$$dir"; \
		cp -f "$(CARGO_TARGET_DIR)/$$target/release/xbin" "$(DIST_DIR)/$$dir/xbin"; \
		cp -f "$(CARGO_TARGET_DIR)/$$target/release/xbin-stub" "$(DIST_DIR)/$$dir/xbin-stub"; \
		cp -f "$(CARGO_TARGET_DIR)/$$target/release/xbin-crypto" "$(DIST_DIR)/$$dir/xbin-crypto"; \
		chmod +x "$(DIST_DIR)/$$dir"/*; \
		tar -czf "$(DIST_DIR)/xbin_$(VERSION)_linux_$$arch.tar.gz" -C "$(DIST_DIR)" "$$dir"; \
		rm -rf "$(DIST_DIR)/$$dir"; \
		echo "  xbin_$(VERSION)_linux_$$arch.tar.gz"; \
	done
	@printf "\n── Checksums ──\n"
	@cd $(DIST_DIR) && sha256sum * > checksums.txt && cat checksums.txt

# Create a GitHub release and upload all dist artifacts.
# Usage: make release VERSION=0.4.0   (tag becomes v0.4.0)
release: dist
	@if [ -z "$(VERSION)" ]; then echo "VERSION required (e.g. make release VERSION=0.4.0)"; exit 1; fi
	@command -v gh >/dev/null 2>&1 || { echo "FAIL: gh (GitHub CLI) not found"; exit 1; }
	@git tag "v$(VERSION)" 2>/dev/null || true
	@git push origin "v$(VERSION)" 2>/dev/null || true
	@gh release create "v$(VERSION)" \
		--repo Tednoob17/x.bin \
		--title "x.bin v$(VERSION)" \
		--notes-file CHANGELOG.md \
		$(DIST_DIR)/*
	@echo "Release v$(VERSION) created: https://github.com/Tednoob17/x.bin/releases/tag/v$(VERSION)"

help:
	@echo "x.bin — package any app into a single self-extracting ELF binary"
	@echo ""
	@echo "Targets:"
	@echo "  make             Build stub + CLI for host"
	@echo "  make install     Build + install CLI to /usr/local/bin or ~/.local/bin"
	@echo "  make stub        Build musl stub for host"
	@echo "  make cli         Build CLI"
	@echo "  make example     Build hello-web.xbin"
	@echo "  make dist        Build all release artifacts (multi-arch)"
	@echo "  make release     Build dist + create GitHub release (VERSION required)"
	@echo "  make lint        Run clippy on all crates"
	@echo "  make fmt         Format all Rust code"
	@echo "  make docs        Build mdbook documentation"
	@echo "  make clean       Clean build artifacts"
	@echo ""
	@echo "Naming convention for release assets:"
	@echo "  xbin_<version>_<os>_<arch>.<ext>"
	@echo ""
	@echo "  linux amd64: xbin_0.4.0_linux_amd64.tar.gz"
	@echo "  linux arm64: xbin_0.4.0_linux_arm64.tar.gz"
	@echo "  checksums:   checksums.txt"
	@echo ""
	@echo "Example: make release VERSION=0.4.0"
