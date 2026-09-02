# AI and edge runtimes

daedalus can package AI and edge workloads, including Ollama models and MCP
tools for agents.

## Gemma via Ollama (on-device AI)

daedalus can package applications that use [Gemma](https://github.com/google/gemma),
Google's open-weights model, running locally via [Ollama](https://ollama.ai) or
[llama.cpp](https://github.com/ggerganov/llama.cpp).

**Use case: Offline code analysis assistant**

1. Write a Python/Node app that uses the `daedalus-core` hidden-dependencies
   analyzer to scan source code for subprocess/dlopen calls
2. The analyzer runs Gemma (via Ollama) locally for LLM-powered classification
3. Package the app with `daedalus build . -o app.daedalus`
4. The `.daedalus` binary embeds Ollama + the Gemma model
5. On first run, Ollama starts and serves the analyzer on `http://127.0.0.1:PORT`
6. The app performs offline code analysis — no internet, no cloud, no cost

```bash
# Build the app with Ollama embedded
daedalus build ./my-code-assistant -o code-assistant.daedalus

# First run: Ollama auto-starts, Gemma model loads
./code-assistant.daedalus

# Then analyze any codebase offline
code-assistant.daedalus scan /path/to/codebase
```

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
