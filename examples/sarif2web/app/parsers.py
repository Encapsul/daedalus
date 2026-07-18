import hashlib
import json


def detect_format(raw_text):
    """Detect if input is SARIF, TruffleHog JSONL, gitleaks JSON, or unknown."""
    stripped = raw_text.strip()
    # SARIF is a single JSON object with "$schema" or "runs"
    if stripped.startswith("{"):
        try:
            data = json.loads(stripped)
            if "runs" in data or "sarif" in data.get("$schema", "").lower():
                return "sarif", data
        except json.JSONDecodeError:
            pass
    # gitleaks JSON is a top-level array with RuleID + Fingerprint
    if stripped.startswith("["):
        try:
            data = json.loads(stripped)
            if isinstance(data, list) and data and "RuleID" in data[0] and "Fingerprint" in data[0]:
                return "gitleaks", data
        except json.JSONDecodeError:
            pass
    # TruffleHog outputs one JSON object per line (JSONL)
    lines = [l for l in stripped.splitlines() if l.strip()]
    if lines:
        try:
            first = json.loads(lines[0])
            if "SourceMetadata" in first and "DetectorName" in first:
                results = [first]
                for line in lines[1:]:
                    if line.strip():
                        results.append(json.loads(line))
                return "trufflehog", results
        except (json.JSONDecodeError, KeyError):
            pass
    # Fallback: try as plain JSON
    try:
        return "sarif", json.loads(stripped)
    except json.JSONDecodeError:
        return "unknown", None


def _extract_source_metadata(metadata):
    """Extract file, line, commit, repo, link from TruffleHog SourceMetadata."""
    info = {"file": "", "line": 0, "commit": "", "repository": "", "link": "", "email": ""}
    if not metadata or "Data" not in metadata:
        return info
    data = metadata["Data"]
    for source_type, fields in data.items():
        if not isinstance(fields, dict):
            continue
        info["file"] = fields.get("file", "")
        info["line"] = fields.get("line", 0)
        info["commit"] = fields.get("commit", "")
        info["repository"] = fields.get("repository", "")
        info["link"] = fields.get("link", "")
        info["email"] = fields.get("email", "")
        break
    return info


def parse_trufflehog(results):
    """Convert TruffleHog JSON results to unified finding format."""
    findings = []
    for result in results:
        detector = result.get("DetectorName", "unknown")
        source_info = _extract_source_metadata(result.get("SourceMetadata"))
        verified = result.get("Verified", False)
        redacted = result.get("Redacted", "")
        extra_data = result.get("ExtraData") or {}

        level = "error" if verified else "warning"

        message_parts = [f"Secret detected by {detector}"]
        if verified:
            message_parts.append("(VERIFIED)")
        if redacted:
            message_parts.append(f": {redacted}")
        message = " ".join(message_parts)

        description_parts = []
        if result.get("DetectorDescription"):
            description_parts.append(result["DetectorDescription"])
        description_parts.append(f"Decoder: {result.get('DecoderName', 'PLAIN')}")
        if source_info["commit"]:
            description_parts.append(f"Commit: {source_info['commit'][:12]}")
        if source_info["email"]:
            description_parts.append(f"Author: {source_info['email']}")
        if source_info["repository"]:
            description_parts.append(f"Repo: {source_info['repository']}")
        for k, v in extra_data.items():
            description_parts.append(f"{k}: {v}")
        description = "\n".join(description_parts)

        fp_source = f"{detector}:{source_info['file']}:{source_info['line']}:{redacted}"
        fingerprint = hashlib.sha256(fp_source.encode()).hexdigest()[:32]

        findings.append({
            "rule_id": f"trufflehog/{detector}",
            "rule_name": detector,
            "level": level,
            "message": message,
            "description": description,
            "short_desc": f"{detector} secret {'(verified)' if verified else '(unverified)'}",
            "help_uri": source_info["link"],
            "file": source_info["file"],
            "start_line": source_info["line"],
            "end_line": source_info["line"],
            "snippet": redacted or "[redacted]",
            "all_locations": [{
                "file": source_info["file"],
                "start_line": source_info["line"],
                "end_line": source_info["line"],
                "start_col": 0,
                "end_col": 0,
                "snippet": redacted or "[redacted]",
            }],
            "code_flows": [],
            "cwes": ["CWE-798"],
            "owasps": [],
            "tags": [
                f"detector:{detector}",
                f"verified:{str(verified).lower()}",
                f"source:{result.get('SourceName', 'unknown')}",
            ],
            "source_tool": "TruffleHog",
            "fingerprint": fingerprint,
            "status": "new",
            "notes": "",
        })
    return findings


def parse_gitleaks(results):
    """Convert gitleaks JSON results to unified finding format."""
    findings = []
    for result in results:
        rule_id = result.get("RuleID", "unknown")
        description = result.get("Description", "")
        file_path = result.get("File", "")
        start_line = result.get("StartLine", 0)
        end_line = result.get("EndLine", 0)
        start_col = result.get("StartColumn", 0)
        end_col = result.get("EndColumn", 0)
        match = result.get("Match", "")
        secret = result.get("Secret", "")
        commit = result.get("Commit", "")
        author = result.get("Author", "")
        email = result.get("Email", "")
        date = result.get("Date", "")
        commit_msg = result.get("Message", "")
        entropy = result.get("Entropy", 0)
        fingerprint = result.get("Fingerprint", "")
        tags = result.get("Tags") or []

        redacted_secret = secret[:4] + "****" + secret[-4:] if len(secret) > 8 else "****"
        redacted_match = match.replace(secret, redacted_secret) if secret and match else match

        level = "error" if entropy > 4.0 else "warning"

        description_parts = [description]
        if commit:
            description_parts.append(f"Commit: {commit[:12]}")
        if author:
            description_parts.append(f"Author: {author}")
        if email:
            description_parts.append(f"Email: {email}")
        if date:
            description_parts.append(f"Date: {date}")
        if commit_msg:
            description_parts.append(f"Commit message: {commit_msg.strip()[:100]}")
        if entropy:
            description_parts.append(f"Entropy: {entropy:.2f}")
        full_description = "\n".join(description_parts)

        findings.append({
            "rule_id": f"gitleaks/{rule_id}",
            "rule_name": description or rule_id,
            "level": level,
            "message": f"Secret detected: {description} in {file_path}:{start_line}",
            "description": full_description,
            "short_desc": description,
            "help_uri": result.get("Link", ""),
            "file": file_path,
            "start_line": start_line,
            "end_line": end_line,
            "snippet": redacted_match,
            "all_locations": [{
                "file": file_path,
                "start_line": start_line,
                "end_line": end_line,
                "start_col": start_col,
                "end_col": end_col,
                "snippet": redacted_match,
            }],
            "code_flows": [],
            "cwes": ["CWE-798"],
            "owasps": [],
            "tags": tags + [f"rule:{rule_id}"],
            "source_tool": "gitleaks",
            "fingerprint": fingerprint or hashlib.sha256(
                f"{rule_id}:{file_path}:{start_line}:{commit}".encode()
            ).hexdigest()[:32],
            "status": "new",
            "notes": "",
        })
    return findings


def parse_sarif(sarif_data):
    """Convert SARIF data to unified finding format."""
    findings = []
    for run in sarif_data.get("runs", []):
        tool_name = run.get("tool", {}).get("driver", {}).get("name", "Unknown")
        rules = {}
        for r in run.get("tool", {}).get("driver", {}).get("rules", []):
            rules[r["id"]] = r
        for ext in run.get("tool", {}).get("extensions", []):
            for r in ext.get("rules", []):
                rules[r["id"]] = r

        for result in run.get("results", []):
            rule_id = result.get("ruleId", "unknown")
            rule = rules.get(rule_id, {})

            level = result.get("level")
            if not level or level == "none":
                level = rule.get("defaultConfiguration", {}).get("level", "warning")

            tags = rule.get("properties", {}).get("tags", [])
            cwes = [t for t in tags if t.startswith("CWE-")]
            owasps = [t for t in tags if "OWASP" in t]

            locations = []
            for loc in result.get("locations", []):
                phys = loc.get("physicalLocation", {})
                artifact = phys.get("artifactLocation", {}).get("uri", "")
                region = phys.get("region", {})
                locations.append({
                    "file": artifact,
                    "start_line": region.get("startLine", 0),
                    "end_line": region.get("endLine", 0),
                    "start_col": region.get("startColumn", 0),
                    "end_col": region.get("endColumn", 0),
                    "snippet": region.get("snippet", {}).get("text", "").strip(),
                })

            code_flows = []
            for cf in result.get("codeFlows", []):
                for tf in cf.get("threadFlows", []):
                    flow_steps = []
                    for tfl in tf.get("locations", []):
                        loc = tfl.get("location", {})
                        phys = loc.get("physicalLocation", {})
                        flow_steps.append({
                            "file": phys.get("artifactLocation", {}).get("uri", ""),
                            "line": phys.get("region", {}).get("startLine", 0),
                            "snippet": phys.get("region", {}).get("snippet", {}).get("text", "").strip(),
                            "message": loc.get("message", {}).get("text", ""),
                        })
                    if flow_steps:
                        code_flows.append(flow_steps)

            findings.append({
                "rule_id": rule_id,
                "rule_name": rule.get("name", rule_id),
                "level": level,
                "message": result.get("message", {}).get("text", ""),
                "description": rule.get("fullDescription", {}).get("text", ""),
                "short_desc": rule.get("shortDescription", {}).get("text", ""),
                "help_uri": rule.get("helpUri", ""),
                "file": locations[0]["file"] if locations else "",
                "start_line": locations[0]["start_line"] if locations else 0,
                "end_line": locations[0]["end_line"] if locations else 0,
                "snippet": locations[0]["snippet"] if locations else "",
                "all_locations": locations,
                "code_flows": code_flows,
                "cwes": cwes,
                "owasps": owasps,
                "tags": tags,
                "source_tool": result.get("properties", {}).get("source_tool", tool_name),
                "fingerprint": result.get("fingerprints", {}).get("matchBasedId/v1", ""),
                "status": "new",
                "notes": "",
            })
    return findings
