import hashlib
from datetime import datetime, timezone

from db import findings_col, projects_col

STATUSES = ["new", "confirmed", "false_positive", "mitigated", "accepted_risk"]


def compute_dedup_hash(finding):
    """Compute a deduplication hash for a finding based on its identity fields."""
    key = (
        f"{finding.get('rule_id', '')}:"
        f"{finding.get('file', '')}:"
        f"{finding.get('start_line', 0)}:"
        f"{finding.get('end_line', 0)}:"
        f"{finding.get('snippet', '')}"
    )
    return hashlib.sha256(key.encode()).hexdigest()


def insert_findings_for_project(project_id, findings, fmt, filename="scan"):
    """Insert findings with deduplication. Returns (inserted_count, duplicates_removed).

    A new finding whose hash matches a soft-deleted one in the project restores
    that document (unsets deleted_at) instead of being treated as a duplicate,
    preserving the original triage history (status, notes, created_at).
    """
    now = datetime.now(timezone.utc)

    for fnd in findings:
        fnd["dedup_hash"] = compute_dedup_hash(fnd)

    duplicates_removed = 0
    restored_count = 0
    if findings:
        new_hashes = [fnd["dedup_hash"] for fnd in findings]
        active_hashes = set()
        deleted_hashes = set()
        for doc in findings_col.find(
            {"project_id": project_id, "dedup_hash": {"$in": new_hashes}},
            {"dedup_hash": 1, "deleted_at": 1},
        ):
            if "deleted_at" in doc:
                deleted_hashes.add(doc["dedup_hash"])
            else:
                active_hashes.add(doc["dedup_hash"])

        hashes_to_restore = deleted_hashes - active_hashes
        if hashes_to_restore:
            result = findings_col.update_many(
                {"project_id": project_id,
                 "dedup_hash": {"$in": list(hashes_to_restore)},
                 "deleted_at": {"$exists": True}},
                {"$set": {"updated_at": now}, "$unset": {"deleted_at": ""}},
            )
            restored_count = result.modified_count

        seen_hashes = active_hashes | hashes_to_restore
        findings = [fnd for fnd in findings if fnd["dedup_hash"] not in seen_hashes]
        duplicates_removed = sum(1 for h in new_hashes if h in active_hashes)

    if findings:
        for fnd in findings:
            fnd["project_id"] = project_id
            fnd["created_at"] = now
            fnd["updated_at"] = now
        findings_col.insert_many(findings)

    inserted_count = len(findings) + restored_count
    projects_col.update_one({"_id": project_id}, {"$inc": {"finding_count": inserted_count}})

    return inserted_count, duplicates_removed


def serialize(doc):
    """Serialize a finding document for JSON response."""
    doc["_id"] = str(doc["_id"])
    doc["project_id"] = str(doc["project_id"])
    if "created_at" in doc:
        doc["created_at"] = doc["created_at"].isoformat()
    if "updated_at" in doc:
        doc["updated_at"] = doc["updated_at"].isoformat()
    return doc


def serialize_project(doc):
    """Serialize a project document for JSON response."""
    doc["_id"] = str(doc["_id"])
    if "uploaded_at" in doc:
        doc["uploaded_at"] = doc["uploaded_at"].isoformat()
    doc.pop("original_sarif", None)
    doc.pop("original_sarifs", None)
    return doc
