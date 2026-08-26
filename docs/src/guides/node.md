# Building a Node.js app

`daedalus` supports Node.js apps end-to-end. It detects a Node.js project by the
presence of a `package.json` file, embeds the Node.js runtime with its shared
libraries, and automatically handles `node_modules` when present.

## Example: hello-node

A minimal Node.js HTTP server:

```
examples/hello-node/
├── app.js
└── package.json
```

**app.js:**
```js
const http = require("http");
const port = parseInt(process.env.PORT || "8080", 10);

const server = http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "text/plain" });
  res.end(`Hello from daedalus + Node.js ${process.version}\n");
});

server.listen(port, () => {
  console.log(`Server listening on http://127.0.0.1:${port}`);
});
```

**package.json:**
```json
{
  "name": "hello-node",
  "main": "app.js"
}
```

Build it:

```bash
daedalus build ./examples/hello-node -o hello-node.daedalus
```

Run it:

```bash
./hello-node.daedalus
# Server listening on http://127.0.0.1:8080
```

## With dependencies (node_modules)

If your app has npm dependencies, they must be installed first (`npm install`)
so that `node_modules/` exists in the project directory. `daedalus` automatically
detects it and sets `NODE_PATH` inside the extracted rootfs.

```bash
cd my-node-app
npm install express
daedalus build . -o my-app.daedalus
```

The build output will confirm:

```
app layer: node_modules from /path/to/my-node-app/node_modules
```

## How it works

1. `detect.rs` detects `package.json` and finds the `node` binary on `$PATH`
2. The ELF analyzer resolves all shared libraries the Node.js runtime depends on (~18 .so files)
3. The interpreter + .so files + /etc form the **runtime layer** (cached)
4. App code + `node_modules` form the **app layer** (rebuilt on every change)
5. The launcher sets `LD_LIBRARY_PATH` and `NODE_PATH`, then `execvp`s node

## Known limitations

- Native addons (`.node` files) are bundled but their transitive `.so`
  dependencies are not yet auto-detected. This works for most JS-only packages.
- Node.js v18+ is tested. The runtime layer caches per exact binary version.
