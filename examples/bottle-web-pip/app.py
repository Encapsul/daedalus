import os
import bottle


@bottle.route("/")
def home():
    return "<h1>Hello from x.bin + pip &#x1F680;</h1>"


bottle.run(host="0.0.0.0", port=int(os.environ.get("PORT", "8080")))

