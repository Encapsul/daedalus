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
		printf "   Add to PATH if needed:\n"; \
		printf "   echo 'export PATH=\"$$HOME/.local/bin:\$$PATH\"' >> ~/.bashrc\n"; \
		printf "   source ~/.bashrc\n"; \
		printf "   You can remove the x.bin repo when done.\n"; \
	fi

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
