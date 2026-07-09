TARGET := x86_64-unknown-linux-musl
TOOLS := /tmp/xbin-stub-target
STUB := $(TOOLS)/$(TARGET)/release/xbin-stub
CRYPTO := $(TOOLS)/$(TARGET)/release/xbin-crypto
XBIN := python3 -m xbin

.PHONY: all stub install example run inspect docs docs-serve clean

all: stub

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

clean:
	cd stub && cargo clean
	rm -rf docs/book
	rm -f *.xbin
	rm -rf ~/.cache/xbin
