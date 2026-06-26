TARGET := x86_64-unknown-linux-musl
STUB := stub/target/$(TARGET)/release/xbin-stub
XBIN := python3 -m xbin

.PHONY: all stub install example run inspect docs docs-serve clean

all: stub

stub:
	cd stub && cargo build --release --target $(TARGET)
	@echo "stub: $$(ls -la $(STUB) | awk '{print $$5}') bytes"

install:
	cd cli && pip install -e .

# Build l'app d'exemple en .xbin (nécessite `stub`).
example: stub
	cd cli && PYTHONPATH=. $(XBIN) build ../examples/hello-web -o ../hello-web.xbin

run:
	./hello-web.xbin

inspect:
	cd cli && PYTHONPATH=. $(XBIN) inspect ../hello-web.xbin

# Documentation (mdbook).
docs:
	cd docs && mdbook build

docs-serve:
	cd docs && mdbook serve --open

clean:
	cd stub && cargo clean
	rm -rf docs/book
	rm -f *.xbin
	rm -rf ~/.cache/xbin
