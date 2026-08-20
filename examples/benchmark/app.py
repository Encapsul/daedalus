import os
import sys

deps_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".deps")
if deps_dir not in sys.path:
    sys.path.insert(0, deps_dir)

import struct
from flask import Flask, request, jsonify

app = Flask(__name__)

MODEL_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), os.environ.get("MODEL_FILE", "Baguettotron-Q4_K_S.gguf"))

GGUF_MAGIC = b"GGUF"

def load_model_info(path):
    with open(path, "rb") as f:
        header = f.read(32)
        if header[:4] != GGUF_MAGIC:
            return {"error": "not a GGUF file"}
        version = struct.unpack("<I", header[4:8])[0]
        f.seek(0, 2)
        total_size = f.tell()
        f.seek(0)
        tensor_data_offset = struct.unpack("<Q", f.read(8))[0]
        return {
            "format": "GGUF",
            "version": version,
            "total_size_bytes": total_size,
            "tensor_data_offset": tensor_data_offset,
        }

MODEL_INFO = load_model_info(MODEL_PATH)

@app.route("/")
def index():
    return jsonify({"status": "ok", "model": os.environ.get("MODEL_FILE", "default"), "info": MODEL_INFO})

@app.route("/infer", methods=["POST"])
def infer():
    prompt = request.json.get("prompt", "") if request.json else ""
    return jsonify({
        "status": "ready",
        "model": os.environ.get("MODEL_FILE", "default"),
        "prompt": prompt,
        "model_info": MODEL_INFO,
        "message": f"Model {os.environ.get('MODEL_FILE', 'default')} ready (simulated)",
    })

if __name__ == "__main__":
    port = int(os.environ.get("PORT", 8000))
    app.run(host="0.0.0.0", port=port)
