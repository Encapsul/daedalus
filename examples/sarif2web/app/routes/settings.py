from datetime import datetime, timezone

from bson import ObjectId
from flask import Blueprint, jsonify, request

from crypto import decrypt, encrypt
from db import semgrep_rules_col, settings_col
from scanner import SETTINGS_KEYS

bp = Blueprint("settings", __name__)


@bp.route("/api/settings", methods=["GET"])
def get_settings():
    result = {}
    for doc in settings_col.find({"key": {"$in": SETTINGS_KEYS}}):
        key = doc["key"]
        raw = doc.get("value", "")
        # Try to decrypt; if it fails, treat as legacy plaintext
        value = decrypt(raw) if raw else ""
        if value:
            masked = "\u2022" * 4 + value[-4:] if len(value) > 4 else "\u2022" * len(value)
            result[key] = {"masked": masked, "is_set": True}
        else:
            result[key] = {"masked": "", "is_set": False}
    for k in SETTINGS_KEYS:
        if k not in result:
            result[k] = {"masked": "", "is_set": False}
    return jsonify(result)


@bp.route("/api/settings", methods=["PUT"])
def put_settings():
    data = request.get_json(force=True) or {}
    for key in SETTINGS_KEYS:
        if key not in data:
            continue
        value = data[key].strip() if isinstance(data[key], str) else ""
        if value:
            encrypted = encrypt(value)
            settings_col.update_one({"key": key}, {"$set": {"key": key, "value": encrypted}}, upsert=True)
        else:
            settings_col.delete_one({"key": key})
    return jsonify({"ok": True})


@bp.route("/api/semgrep-rules", methods=["GET"])
def list_semgrep_rules():
    docs = list(semgrep_rules_col.find().sort("created_at", -1))
    for d in docs:
        d["_id"] = str(d["_id"])
        if "created_at" in d:
            d["created_at"] = d["created_at"].isoformat()
    return jsonify(docs)


@bp.route("/api/semgrep-rules", methods=["POST"])
def add_semgrep_rule():
    data = request.get_json(force=True)
    name = (data.get("name") or "").strip()
    content = (data.get("content") or "").strip()
    if not name:
        return jsonify({"error": "name is required"}), 400
    if not content:
        return jsonify({"error": "YAML content is required"}), 400
    if "rules:" not in content and "- id:" not in content:
        return jsonify({"error": "content doesn't look like a valid Semgrep rule (missing 'rules:' or '- id:')"}), 400
    doc = {
        "name": name,
        "content": content,
        "created_at": datetime.now(timezone.utc),
    }
    rid = semgrep_rules_col.insert_one(doc).inserted_id
    return jsonify({"_id": str(rid), "name": name})


@bp.route("/api/semgrep-rules/<rule_id>", methods=["DELETE"])
def delete_semgrep_rule(rule_id):
    try:
        rid = ObjectId(rule_id)
    except Exception:
        return jsonify({"error": "invalid id"}), 400
    semgrep_rules_col.delete_one({"_id": rid})
    return jsonify({"ok": True})
