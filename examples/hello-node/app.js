const http = require("http");
const port = parseInt(process.env.PORT || "8080", 10);

const server = http.createServer((req, res) => {
  const msg = `Hello from x.bin + Node.js ${process.version}\n`;
  res.writeHead(200, { "Content-Type": "text/plain" });
  res.end(msg);
});

server.listen(port, () => {
  console.log(`Server listening on http://127.0.0.1:${port}`);
});
