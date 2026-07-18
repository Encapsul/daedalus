from datetime import datetime, timezone

from bson import ObjectId
from flask import Blueprint, jsonify, request

from db import findings_col, projects_col
from helpers import serialize_project

bp = Blueprint("projects", __name__)


@bp.route("/api/projects", methods=["GET"])
def list_projects():
    docs = list(projects_col.find({}, {"original_sarif": 0, "original_sarifs": 0}).sort("uploaded_at", -1))
    return jsonify([serialize_project(d) for d in docs])


@bp.route("/api/projects", methods=["POST"])
def create_project():
    data = request.get_json(force=True)
    name = (data.get("name") or "").strip()
    if not name:
        return jsonify({"error": "name is required"}), 400
    now = datetime.now(timezone.utc)
    project = {
        "name": name,
        "uploaded_at": now,
        "finding_count": 0,
        "source_format": None,
        "original_sarifs": [],
    }
    pid = projects_col.insert_one(project).inserted_id
    return jsonify({"_id": str(pid), "name": name})


@bp.route("/api/projects/<project_id>", methods=["DELETE"])
def delete_project(project_id):
    try:
        pid = ObjectId(project_id)
    except Exception:
        return jsonify({"error": "invalid id"}), 400
    findings_col.delete_many({"project_id": pid})
    projects_col.delete_one({"_id": pid})
    return jsonify({"ok": True})


@bp.route("/api/projects/bulk-delete", methods=["POST"])
def bulk_delete_projects():
    data = request.get_json(force=True)
    ids = data.get("ids", [])
    if not ids:
        return jsonify({"error": "no ids provided"}), 400
    try:
        oids = [ObjectId(i) for i in ids]
    except Exception:
        return jsonify({"error": "invalid id in list"}), 400
    findings_col.delete_many({"project_id": {"$in": oids}})
    result = projects_col.delete_many({"_id": {"$in": oids}})
    return jsonify({"ok": True, "deleted": result.deleted_count})
