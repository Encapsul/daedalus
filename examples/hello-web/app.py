"""App web d'exemple — serveur HTTP stdlib, zéro dépendance externe.

Démontre le cas d'usage cible de xbin : un serveur web headless qui démarre
avec ./hello-web.xbin et sert sur localhost.
"""

import os
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(os.environ.get("PORT", "8080"))


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        body = (
            "<!doctype html><html><head><meta charset=utf-8>"
            "<title>xbin</title></head><body style='font-family:system-ui;"
            "max-width:40rem;margin:4rem auto'>"
            "<h1>Hello from xbin 🚀</h1>"
            f"<p>Ce serveur tourne depuis un seul fichier exécutable.</p>"
            f"<p>Python {sys.version.split()[0]} — PID {os.getpid()}</p>"
            "</body></html>"
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass  # silencieux


def main():
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"Server listening on http://127.0.0.1:{PORT}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nshutting down", flush=True)


if __name__ == "__main__":
    main()
