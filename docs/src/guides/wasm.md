# WASM / WASI runtime

daedalus can package WebAssembly modules and run them via Wasmtime with WASI
preview1 or the component model.

## Build

```bash
# Basic WASM
daedalus build ./my-wasm-app -o my-wasm.daedalus --wasm

# With a custom Wasmtime binary
daedalus build ./my-wasm-app -o my-wasm.daedalus \
  --wasm --wasmtime-path /usr/local/bin/wasmtime

# WASI preview1
daedalus build ./my-wasm-app -o my-wasm.daedalus \
  --wasm --wasi

# Component model
daedalus build ./my-wasm-app -o my-wasm.daedalus \
  --wasm --component-model
```

## Detection

daedalus auto-detects WASM apps by:

- `.wasm` or `.wat` extension in the app directory
- Presence of a `wasmtime` binary in PATH (or the path given by `--wasmtime-path`)

## Options

| Flag | Description |
|---|---|
| `--wasm` | Treat the app as a WASM module |
| `--wasmtime-path <PATH>` | Path to the Wasmtime binary |
| `--wasi` | Enable WASI preview1 |
| `--component-model` | Enable the WASI component model |

## Runtime

At runtime, the launcher invokes:

```
wasmtime run --wasi <module>.wasm <args...>
```

or for the component model:

```
wasmtime run --component-model <module>.wasm <args...>
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | WASM module exited successfully |
| `1` | Execution failure |
