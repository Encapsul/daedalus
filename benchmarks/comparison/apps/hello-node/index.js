const http = require('http');
const port = parseInt(process.env.PORT || '8080', 10);
const server = http.createServer((req, res) => {
  res.writeHead(200, { 'content-type': 'text/plain' });
  res.end('hello from x.bin benchmark\n');
});
server.listen(port, '0.0.0.0', () => console.log('listening on ' + port));
