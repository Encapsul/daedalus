"""App web d'exemple avec une dépendance tierce (bottle).

Démontre l'embarquement des site-packages d'un .venv : `bottle` n'est PAS dans
la stdlib, il est installé dans .venv/ et xbin doit l'inclure dans le rootfs.
"""

import os
import sys

from bottle import Bottle, response

app = Bottle()


@app.route("/")
def index():
    response.content_type = "text/html; charset=utf-8"
    return (
        "<!doctype html><html><head><meta charset=utf-8><title>xbin + bottle</title>"
        "</head><body style='font-family:system-ui;max-width:40rem;margin:4rem auto'>"
        "<h1>Hello from bottle, packagé par xbin 🍾</h1>"
        "<p>Ce serveur utilise <code>bottle</code>, une dépendance tierce "
        "embarquée depuis le .venv — pas la stdlib.</p>"
        f"<p>Python {sys.version.split()[0]} — PID {os.getpid()}</p>"
        "</body></html>"
    )


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8080"))
    print(f"Server listening on http://127.0.0.1:{port}", flush=True)
    app.run(host="127.0.0.1", port=port, quiet=True)
