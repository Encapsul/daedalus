HOST_ARCH ?= $(shell uname -m)
TARGET ?= $(HOST_ARCH)-unknown-linux-musl
TOOLS := /tmp/xbin-stub-target
STUB := $(TOOLS)/$(TARGET)/release/xbin-stub
CRYPTO := $(TOOLS)/$(TARGET)/release/xbin-crypto
CLI := target/release/xbin

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
	@echo "stub:   $$(ls -la $(STUB) | awk '{print $$5}') bytes"
	@echo "crypto: $$(ls -la $(CRYPTO) | awk '{print $$5}') bytes"

# Build the Rust CLI.
cli:
	cargo build --release -p xbin-cli
	@echo "cli:    $$(ls -la $(CLI) | awk '{print $$5}') bytes"

# Install the CLI to ~/.local/bin.
install: cli
	install -d ~/.local/bin
	install -m 755 $(CLI) ~/.local/bin/xbin
	@echo ""
	@echo "installed: ~/.local/bin/xbin"
	@echo "NOTE: if 'xbin' is not found, add ~/.local/bin to PATH:"
	@echo "  export PATH=\"\$$HOME/.local/bin:\$$PATH\""

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
