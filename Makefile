HOST_ARCH ?= $(shell uname -m)
TARGET ?= $(HOST_ARCH)-unknown-linux-musl
TOOLS := /tmp/xbin-stub-target
STUB := $(TOOLS)/$(TARGET)/release/xbin-stub
CRYPTO := $(TOOLS)/$(TARGET)/release/xbin-crypto
CLI := target/release/xbin
STUB_ALT := target/$(TARGET)/release/xbin-stub
CRYPTO_ALT := target/$(TARGET)/release/xbin-crypto

.PHONY: all preflight stub cli install example run inspect docs docs-serve lint fmt clean

all: stub cli

# Verify all prerequisites are installed before building.
preflight:
	@echo "checking prerequisites..."
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

.PHONY: install install-system install-local

# Build everything
all: stub cli

# System-wide installation (/usr/local/bin, admin required, persists after repo removal)
install-system:
	@printf "\nBuilding and installing x.bin to /usr/local/bin...\n"
	sudo mkdir -p /usr/local/bin
	sudo cargo build --release -p xbin-cli
	sudo cp target/release/xbin /usr/local/bin/xbin
	sudo chmod +x /usr/local/bin/xbin
	@echo ""
	@echo "✅ Successfully installed to /usr/local/bin/xbin"
	@echo "   Verify: /usr/local/bin/xbin --version"
	@echo ""
	@echo "🚨 IMPORTANT: You must add /usr/local/bin to your PATH"
	@echo "   Add to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
	@echo "   echo 'export PATH=\"/usr/local/bin:\\$PATH\"' >> ~/.bashrc"
	@echo "   source ~/.bashrc"
	@echo "   or run now: export PATH=\"/usr/local/bin:\\$PATH\""
	@echo ""
	@echo "📋 You can now remove the x.bin repository entirely!"

# User-level installation (~/.local/bin, no sudo, portable)
install-local: cli
	@printf "\nInstalling x.bin to ~/.local/bin (no admin required)...\n"
	mkdir -p ~/.local/bin
	cp target/release/xbin ~/.local/bin/xbin
	chmod +x ~/.local/bin/xbin
	@echo ""
	@echo "✅ Successfully installed to ~/.local/bin/xbin"
	@echo "   Verify: ~/.local/bin/xbin --version"
	@echo ""
	@echo "🔧 Add to your PATH (optional but recommended):"
	@echo "   echo 'export PATH=\"$HOME/.local/bin:\\$PATH\"' >> ~/.bashrc"
	@echo "   source ~/.bashrc"
	@echo "   or run now: export PATH=\"$HOME/.local/bin:\\$PATH\""
	@echo ""
	@echo "🚀 You're ready to go! Remove x.bin repo if desired."

# Hybrid install: tries system, falls back to user level
install:
	@if [ "$(id -u)" -eq 0 ]; then \
		make install-system; \
	else \
		make install-local; \
	fi

# Legacy install target (user level)
install-legacy: install-local

# Build the Rust stub (statically linked musl ELF).

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
	rm -rf ~/.cache/xbin
