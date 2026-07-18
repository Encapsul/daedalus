import os
from datetime import datetime, timezone

from bson import ObjectId
from flask import Blueprint, jsonify, request

from db import projects_col, scans_col
from scanner import (
    SCAN_REPO_BASE,
    SCANNERS,
    _validate_repo_url,
    get_settings_dict,
    run_scan,
    scan_executor,
)

bp = Blueprint("scans", __name__)

TOKEN_LABELS = {"snyk_token": "Snyk", "semgrep_token": "Semgrep"}


@bp.route("/api/scanners", methods=["GET"])
def list_scanners():
    """List available scanners and their descriptions."""
    result = []
    for name, info in SCANNERS.items():
        result.append({
            "name": name,
            "description": info["description"],
            "output_format": info["output_format"],
        })
    return jsonify(result)


@bp.route("/api/scans", methods=["POST"])
def create_scan():
    data = request.get_json(force=True)

    project_id_str = data.get("project_id")
    tools_raw = data.get("tools") or data.get("tool")
    repo_url = (data.get("repo_url") or "").strip()
    branch = (data.get("branch") or "").strip() or None

    if not project_id_str:
        return jsonify({"error": "project_id is required"}), 400
    try:
        project_id = ObjectId(project_id_str)
    except Exception:
        return jsonify({"error": "invalid project_id"}), 400

    if not projects_col.find_one({"_id": project_id}):
        return jsonify({"error": "project not found"}), 404

    if not _validate_repo_url(repo_url):
        return jsonify({"error": "invalid repo URL — only http:// and https:// URLs are allowed"}), 400

    if isinstance(tools_raw, list):
        tools_to_run = tools_raw
    elif tools_raw == "all":
        tools_to_run = list(SCANNERS.keys())
    elif tools_raw:
        tools_to_run = [tools_raw]
    else:
        return jsonify({"error": "at least one scanner is required"}), 400

    for t in tools_to_run:
        if t not in SCANNERS:
            return jsonify({"error": f"unknown scanner: {t}. Available: {', '.join(SCANNERS.keys())}"}), 400

    settings = get_settings_dict()
    missing = []
    for t in tools_to_run:
        req = SCANNERS[t].get("requires_token")
        if req and not settings.get(req):
            missing.append(TOKEN_LABELS.get(req, req))
    if missing:
        names = ", ".join(missing)
        return jsonify({"error": f"Missing token(s) for {names}. Configure them in Settings (gear icon)."}), 400

    os.makedirs(SCAN_REPO_BASE, exist_ok=True)

    scan_ids = []
    now = datetime.now(timezone.utc)
    for t in tools_to_run:
        scan_doc = {
            "project_id": project_id,
            "tool": t,
            "status": "pending",
            "repo_url": repo_url,
            "branch": branch,
            "config": data.get("config", {}),
            "started_at": now,
            "completed_at": None,
            "duration_seconds": None,
            "log": "",
            "error": "",
            "findings_count": 0,
            "duplicates_count": 0,
            "created_at": now,
        }
        sid = scans_col.insert_one(scan_doc).inserted_id
        scan_ids.append(str(sid))
        scan_executor.submit(run_scan, sid)

    return jsonify({"ids": scan_ids, "status": "pending", "count": len(scan_ids)})


@bp.route("/api/scans", methods=["GET"])
def list_scans():
    project_id = request.args.get("project_id")
    if not project_id:
        return jsonify({"error": "project_id required"}), 400
    try:
        pid = ObjectId(project_id)
    except Exception:
        return jsonify({"error": "invalid project_id"}), 400

    docs = list(scans_col.find({"project_id": pid}).sort("created_at", -1))
    result = []
    for d in docs:
        d["_id"] = str(d["_id"])
        d["project_id"] = str(d["project_id"])
        for field in ("created_at", "started_at", "completed_at"):
            if d.get(field):
                d[field] = d[field].isoformat()
        result.append(d)
    return jsonify(result)


@bp.route("/api/scans/<scan_id>", methods=["GET"])
def get_scan(scan_id):
    try:
        sid = ObjectId(scan_id)
    except Exception:
        return jsonify({"error": "invalid id"}), 400

    d = scans_col.find_one({"_id": sid})
    if not d:
        return jsonify({"error": "not found"}), 404

    d["_id"] = str(d["_id"])
    d["project_id"] = str(d["project_id"])
    for field in ("created_at", "started_at", "completed_at"):
        if d.get(field):
            d[field] = d[field].isoformat()
    return jsonify(d)


@bp.route("/api/scans/<scan_id>", methods=["DELETE"])
def delete_scan(scan_id):
    try:
        sid = ObjectId(scan_id)
    except Exception:
        return jsonify({"error": "invalid id"}), 400
    scans_col.delete_one({"_id": sid})
    return jsonify({"ok": True})
