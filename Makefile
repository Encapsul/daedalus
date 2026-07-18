HOST_ARCH ?= $(shell uname -m)
TARGET ?= $(HOST_ARCH)-unknown-linux-musl
TOOLS := /tmp/xbin-stub-target
STUB := $(TOOLS)/$(TARGET)/release/xbin-stub
CRYPTO := $(TOOLS)/$(TARGET)/release/xbin-crypto
XBIN := python3 -m xbin

.PHONY: all preflight stub install example run inspect docs docs-serve lint lint-rust lint-python fmt fmt-rust fmt-python clean

all: stub

# Verify all prerequisites are installed before building.
preflight:
	@echo "checking prerequisites..."
	@command -v python3 >/dev/null 2>&1 || { echo "FAIL: python3 not found (install python3)"; exit 1; }
	@command -v pip3 >/dev/null 2>&1 || python3 -m pip --version >/dev/null 2>&1 || { echo "FAIL: pip not found (install python3-pip)"; exit 1; }
	@command -v cargo >/dev/null 2>&1 || { echo "FAIL: cargo not found (install Rust: https://rustup.rs)"; exit 1; }
	@command -v rustc >/dev/null 2>&1 || { echo "FAIL: rustc not found (install Rust: https://rustup.rs)"; exit 1; }
	@rustup target list --installed 2>/dev/null | grep -q "$(HOST_ARCH)-unknown-linux-musl" || { echo "FAIL: musl target missing (run: rustup target add $(HOST_ARCH)-unknown-linux-musl)"; exit 1; }
	@command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || { echo "FAIL: C compiler not found (install gcc or musl-tools)"; exit 1; }
	@command -v zstd >/dev/null 2>&1 || { echo "FAIL: zstd not found (install: apt install zstd)"; exit 1; }
	@python3 -c "import sys; sys.exit(0 if sys.version_info >= (3,10) else 1)" 2>/dev/null || { echo "FAIL: Python >= 3.10 required"; exit 1; }
	@echo "all prerequisites OK"

# Build the Rust binaries (statically linked musl ELF).
stub:
	cd stub && cargo build --release --target $(TARGET)
	@echo "stub:   $$(ls -la $(STUB) | awk '{print $$5}') bytes"
	@echo "crypto: $$(ls -la $(CRYPTO) | awk '{print $$5}') bytes"

# Install the CLI via pip editable install.
install:
	cd cli && pip install -e .

# Build the hello-web example .xbin (requires `stub`).
example: stub
	cd cli && PYTHONPATH=. $(XBIN) build ../examples/hello-web -o ../hello-web.xbin

# Run the hello-web .xbin.
run:
	./hello-web.xbin

# Inspect the hello-web .xbin.
inspect:
	cd cli && PYTHONPATH=. $(XBIN) inspect ../hello-web.xbin

# Build mdbook documentation.
docs:
	cd docs && mdbook build

docs-serve:
	cd docs && mdbook serve --open

lint: lint-rust lint-python

lint-rust:
	cd stub && cargo clippy -- -D warnings

lint-python:
	cd cli && python -m ruff check xbin/
	cd cli && python -m black --check xbin/

fmt: fmt-rust fmt-python

fmt-rust:
	cd stub && cargo fmt

fmt-python:
	cd cli && python -m black xbin/
	cd cli && python -m ruff check --fix xbin/

clean:
	cd stub && cargo clean
	rm -rf docs/book
	rm -f *.xbin
	rm -rf ~/.cache/xbin
