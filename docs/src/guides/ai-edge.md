# AI and edge runtimes

daedalus can package AI and edge workloads, including Ollama models and MCP
tools for agents.

## Ollama

Package an Ollama-based AI app:

```bash
daedalus build ./my-ollama-app -o my-ollama-app.daedalus
```

daedalus detects:

- `ollama` in `package.json` scripts or dependencies
- `Modelfile` or `models/` directory

The binary embeds the Ollama binary and model files. At runtime, it starts the
Ollama server and serves the app on the configured port.

## MCP tools for agents

daedalus can expose Model Context Protocol (MCP) tools:

```bash
daedalus build ./my-agent -o my-agent.daedalus \
  --mcp-tools ./tools/
```

MCP tools are embedded as standalone binaries or scripts and exposed over
stdin/stdout JSON-RPC.

## Edge LLM deployment

For air-gapped or edge environments:

```bash
daedalus build ./my-llm -o my-llm.daedalus \
  --encrypt \
  --persist ./models
```

The `--persist` flag ensures large model files are stored outside the binary
and survive updates.

## Options

| Flag | Description |
|---|---|
| `--embed-interpreter <PATH>` | Embed a custom runtime binary |
| `--persist <PATH>` | Preserve a data directory across runs |
| `--encrypt` | Encrypt the payload (for sensitive model weights) |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | App exited successfully |
| `1` | Extraction, model load, or runtime failure |
