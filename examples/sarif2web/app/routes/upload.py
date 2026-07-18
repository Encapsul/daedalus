import json

from bson import ObjectId
from flask import Blueprint, Response, jsonify, request

from db import findings_col, projects_col
from helpers import insert_findings_for_project
from parsers import detect_format, parse_gitleaks, parse_sarif, parse_trufflehog

bp = Blueprint("upload", __name__)


ALLOWED_EXTENSIONS = {"sarif", "json", "jsonl"}


@bp.route("/api/upload", methods=["POST"])
def upload():
    f = request.files.get("file")
    if not f:
        return jsonify({"error": "no file"}), 400

    # Validate file extension
    ext = (f.filename or "").rsplit(".", 1)[-1].lower() if "." in (f.filename or "") else ""
    if ext not in ALLOWED_EXTENSIONS:
        return jsonify({"error": f"unsupported file extension (allowed: {', '.join(sorted(ALLOWED_EXTENSIONS))})"}), 400

    project_id_str = request.form.get("project_id")
    if not project_id_str:
        return jsonify({"error": "project_id is required"}), 400
    try:
        project_id = ObjectId(project_id_str)
    except Exception:
        return jsonify({"error": "invalid project_id"}), 400

    project = projects_col.find_one({"_id": project_id})
    if not project:
        return jsonify({"error": "project not found"}), 404

    try:
        raw_text = f.read().decode("utf-8")
    except UnicodeDecodeError:
        return jsonify({"error": "file must be valid UTF-8"}), 400
    except Exception:
        return jsonify({"error": "cannot read file"}), 400

    if not raw_text.strip():
        return jsonify({"error": "file is empty"}), 400

    fmt, parsed = detect_format(raw_text)

    if fmt == "sarif":
        findings = parse_sarif(parsed)
        original_data = parsed
    elif fmt == "trufflehog":
        findings = parse_trufflehog(parsed)
        original_data = {"_trufflehog_results": parsed, "_format": "trufflehog"}
    elif fmt == "gitleaks":
        findings = parse_gitleaks(parsed)
        original_data = {"_gitleaks_results": parsed, "_format": "gitleaks"}
    else:
        return jsonify({"error": "unsupported format (expected SARIF, TruffleHog or gitleaks JSON)"}), 400

    inserted, duplicates_removed = insert_findings_for_project(project_id, findings, fmt, f.filename or "unknown")

    update_ops = {
        "$push": {"original_sarifs": {"filename": f.filename or "unknown", "format": fmt, "data": original_data}},
    }
    if not project.get("source_format"):
        update_ops.setdefault("$set", {})["source_format"] = fmt
    projects_col.update_one({"_id": project_id}, update_ops)

    return jsonify({
        "project_id": str(project_id),
        "count": inserted,
        "duplicates_removed": duplicates_removed,
        "format": fmt,
    })


SARIF_SCHEMA = "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json"


def _finding_to_sarif_result(fnd, tool_name):
    locations = []
    for loc in fnd.get("all_locations") or []:
        region = {
            "startLine": loc.get("start_line") or 1,
            "endLine": loc.get("end_line") or loc.get("start_line") or 1,
        }
        if loc.get("start_col"):
            region["startColumn"] = loc["start_col"]
        if loc.get("end_col"):
            region["endColumn"] = loc["end_col"]
        if loc.get("snippet"):
            region["snippet"] = {"text": loc["snippet"]}
        locations.append({
            "physicalLocation": {
                "artifactLocation": {"uri": loc.get("file", "")},
                "region": region,
            }
        })

    if not locations and fnd.get("file"):
        region = {
            "startLine": fnd.get("start_line") or 1,
            "endLine": fnd.get("end_line") or fnd.get("start_line") or 1,
        }
        if fnd.get("snippet"):
            region["snippet"] = {"text": fnd["snippet"]}
        locations.append({
            "physicalLocation": {
                "artifactLocation": {"uri": fnd["file"]},
                "region": region,
            }
        })

    code_flows = []
    for flow in fnd.get("code_flows") or []:
        tf_locations = []
        for step in flow:
            region = {"startLine": step.get("line") or 1}
            if step.get("snippet"):
                region["snippet"] = {"text": step["snippet"]}
            tf_locations.append({
                "location": {
                    "physicalLocation": {
                        "artifactLocation": {"uri": step.get("file", "")},
                        "region": region,
                    },
                    "message": {"text": step.get("message", "")},
                }
            })
        if tf_locations:
            code_flows.append({"threadFlows": [{"locations": tf_locations}]})

    result = {
        "ruleId": fnd.get("rule_id", "unknown"),
        "level": fnd.get("level", "warning"),
        "message": {"text": fnd.get("message", "")},
        "locations": locations,
        "properties": {
            "review_status": fnd.get("status", "new"),
            "review_notes": fnd.get("notes", ""),
            "cwes": fnd.get("cwes", []),
            "owasps": fnd.get("owasps", []),
            "source_tool": tool_name,
        },
    }
    if code_flows:
        result["codeFlows"] = code_flows
    if fnd.get("fingerprint"):
        result["fingerprints"] = {"matchBasedId/v1": fnd["fingerprint"]}
    return result


def _build_sarif_run(tool_name, findings):
    rules = {}
    for fnd in findings:
        rid = fnd.get("rule_id", "unknown")
        if rid in rules:
            continue
        tag_list = list(fnd.get("tags") or []) + list(fnd.get("cwes") or []) + list(fnd.get("owasps") or [])
        rule = {
            "id": rid,
            "name": fnd.get("rule_name") or rid,
            "shortDescription": {"text": fnd.get("short_desc", "")},
            "fullDescription": {"text": fnd.get("description", "")},
            "defaultConfiguration": {"level": fnd.get("level", "warning")},
            "properties": {"tags": list(dict.fromkeys(tag_list))},
        }
        if fnd.get("help_uri"):
            rule["helpUri"] = fnd["help_uri"]
        rules[rid] = rule

    return {
        "tool": {"driver": {"name": tool_name, "rules": list(rules.values())}},
        "results": [_finding_to_sarif_result(f, tool_name) for f in findings],
    }


@bp.route("/api/export/<project_id>", methods=["GET"])
def export_sarif(project_id):
    try:
        pid = ObjectId(project_id)
    except Exception:
        return jsonify({"error": "invalid id"}), 400

    project = projects_col.find_one({"_id": pid})
    if not project:
        return jsonify({"error": "not found"}), 404

    findings = list(findings_col.find({"project_id": pid, "deleted_at": {"$exists": False}}))

    by_tool = {}
    for fnd in findings:
        by_tool.setdefault(fnd.get("source_tool") or "Unknown", []).append(fnd)

    runs = [_build_sarif_run(tool, items) for tool, items in sorted(by_tool.items())]

    export_data = {
        "$schema": SARIF_SCHEMA,
        "version": "2.1.0",
        "runs": runs,
    }

    filename = project["name"]
    if not filename.endswith(".sarif") and not filename.endswith(".json"):
        filename += ".sarif"

    return Response(
        json.dumps(export_data, indent=2),
        mimetype="application/json",
        headers={"Content-Disposition": f"attachment; filename={filename}"},
    )
