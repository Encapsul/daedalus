import ipaddress
import json
import os
import re
import shutil
import socket
import subprocess
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from urllib.parse import urlparse

from crypto import decrypt
from db import projects_col, scans_col, settings_col
from helpers import insert_findings_for_project
from parsers import detect_format, parse_gitleaks, parse_sarif, parse_trufflehog

SCANNERS = {
    "semgrep":    {"cmd": "semgrep", "output_format": "sarif",      "description": "SAST scanner for 30+ languages. Uses 'semgrep ci' with Semgrep Cloud rules when token is set, otherwise falls back to 'semgrep scan'.", "requires_token": None},
    "snyk":       {"cmd": "snyk", "output_format": "sarif",         "description": "Snyk Code — finds vulnerabilities using semantic analysis. Requires a Snyk token (configure in Settings).", "requires_token": "snyk_token"},
    "codeql":     {"cmd": "codeql", "output_format": "sarif",       "description": "GitHub CodeQL — deep semantic code analysis engine."},
    "trufflehog": {"cmd": "trufflehog", "output_format": "trufflehog", "description": "Scans for leaked secrets, API keys, and credentials in git history."},
    "gitleaks":   {"cmd": "gitleaks", "output_format": "gitleaks",  "description": "Fast secret scanner using regex and entropy detection."},
}

SETTINGS_KEYS = ["snyk_token", "github_token", "semgrep_token"]

SCAN_REPO_BASE = "/tmp/scan_repos"

scan_executor = ThreadPoolExecutor(max_workers=2)


def _validate_repo_url(url):
    """Validate that a repo URL is a safe public HTTP(S) Git URL."""
    if not url or not isinstance(url, str):
        return False
    url = url.strip()
    if not re.match(r'^https?://', url, re.IGNORECASE):
        return False

    try:
        parsed = urlparse(url)
        hostname = parsed.hostname or ""
    except Exception:
        return False

    if not hostname:
        return False

    # Resolve hostname to IP and block private/reserved ranges
    try:
        addr_info = socket.getaddrinfo(hostname, None)
        for _, _, _, _, sockaddr in addr_info:
            ip = ipaddress.ip_address(sockaddr[0])
            if ip.is_private or ip.is_loopback or ip.is_reserved or ip.is_link_local:
                return False
    except (socket.gaierror, ValueError):
        return False

    return True


def _update_scan(scan_id, **fields):
    """Update scan document fields."""
    scans_col.update_one({"_id": scan_id}, {"$set": fields})


def get_settings_dict():
    """Return a dict of all settings {key: decrypted_value}."""
    result = {}
    for doc in settings_col.find({"key": {"$in": SETTINGS_KEYS}}):
        raw = doc.get("value", "")
        result[doc["key"]] = decrypt(raw) or raw if raw else ""
    return result


def _get_scanner_env(settings=None):
    """Build environment dict with tokens from settings collection."""
    if settings is None:
        settings = get_settings_dict()
    env = os.environ.copy()
    if settings.get("snyk_token"):
        env["SNYK_TOKEN"] = settings["snyk_token"]
        subprocess.run(
            ["snyk", "auth", settings["snyk_token"], "--auth-type=token"],
            capture_output=True, text=True, timeout=30, env=env
        )
    if settings.get("github_token"):
        env["GITHUB_TOKEN"] = settings["github_token"]
    return env


def _run_scanner_command(tool, repo_dir, output_file, env=None, custom_configs=None):
    """Run the appropriate scanner command and return (success, stdout, stderr)."""
    timeout = 1800
    run_kw = dict(capture_output=True, text=True, timeout=timeout)
    if env is not None:
        run_kw["env"] = env

    try:
        if tool == "semgrep":
            extra_configs = [f"--config={p}" for p in (custom_configs or [])]
            semgrep_token = get_settings_dict().get("semgrep_token")
            # `semgrep ci` rejects extra --config flags while logged in, so fall
            # back to `semgrep scan` whenever custom rules are provided.
            use_ci = bool(semgrep_token) and not extra_configs
            if use_ci:
                ci_env = dict(env or os.environ.copy())
                ci_env["SEMGREP_APP_TOKEN"] = semgrep_token
                ci_kw = dict(run_kw)
                ci_kw["env"] = ci_env
                ci_kw["cwd"] = repo_dir
                cmd = ["semgrep", "ci", "--sarif", f"--sarif-output={output_file}", "--timeout=300"]
                result = subprocess.run(cmd, **ci_kw)
                combined = (result.stdout or "") + (result.stderr or "")
                if "API token not valid" in combined or "HTTP 401" in combined:
                    use_ci = False
            if not use_ci:
                subprocess.run(["semgrep", "logout"], capture_output=True, text=True, timeout=10)
                cmd = ["semgrep", "scan", "--config=p/default"] + extra_configs + [
                       "--metrics=off", "--timeout=300", "--sarif", f"--sarif-output={output_file}", repo_dir]
                result = subprocess.run(cmd, **run_kw)
            return result.returncode in (0, 1), result.stdout, result.stderr

        elif tool == "snyk":
            cmd = ["snyk", "code", "test", f"--sarif-file-output={output_file}", repo_dir]
            result = subprocess.run(cmd, **run_kw)
            return result.returncode in (0, 1), result.stdout, result.stderr

        elif tool == "codeql":
            db_dir = os.path.join(os.path.dirname(output_file), "codeql-db")
            lang = "javascript"
            for ext, l in [(".py", "python"), (".java", "java"), (".go", "go"),
                           (".rb", "ruby"), (".cs", "csharp"), (".cpp", "cpp"),
                           (".c", "cpp"), (".js", "javascript"), (".ts", "javascript")]:
                for root, dirs, files in os.walk(repo_dir):
                    if any(f.endswith(ext) for f in files):
                        lang = l
                        break
                else:
                    continue
                break

            result = subprocess.run(
                ["codeql", "database", "create", db_dir, f"--source-root={repo_dir}", f"--language={lang}", "--overwrite"],
                **run_kw
            )
            if result.returncode != 0:
                return False, result.stdout, result.stderr

            result = subprocess.run(
                ["codeql", "database", "analyze", db_dir, f"--format=sarif-latest", f"--output={output_file}"],
                **run_kw
            )
            return result.returncode == 0, result.stdout, result.stderr

        elif tool == "trufflehog":
            cmd = ["trufflehog", "filesystem", repo_dir, "--json"]
            result = subprocess.run(cmd, **run_kw)
            with open(output_file, "w") as f:
                f.write(result.stdout)
            return True, result.stdout, result.stderr

        elif tool == "gitleaks":
            cmd = ["gitleaks", "detect", f"--source={repo_dir}", "--report-format=json", f"--report-path={output_file}"]
            result = subprocess.run(cmd, **run_kw)
            return result.returncode in (0, 1), result.stdout, result.stderr

        else:
            return False, "", f"Unknown scanner: {tool}"

    except FileNotFoundError:
        return False, "", f"Scanner binary '{tool}' not found in PATH. Is it installed?"


def _repo_name_from_url(url):
    """Extract a safe directory name from a git URL."""
    path = re.sub(r'^https?://', '', url).rstrip('/')
    path = re.sub(r'\.git$', '', path)
    parts = path.split('/')
    name = '_'.join(parts[-2:]) if len(parts) >= 2 else parts[-1]
    name = re.sub(r'[^a-zA-Z0-9_\-]', '_', name)
    return name or "repo"


def run_scan(scan_id):
    """Execute a scan in a background thread."""
    scan = scans_col.find_one({"_id": scan_id})
    if not scan:
        return

    tool = scan["tool"]
    repo_url = scan["repo_url"]
    branch = scan.get("branch")
    project_id = scan["project_id"]
    scanner_info = SCANNERS.get(tool)

    tmpdir = None
    try:
        settings = get_settings_dict()
        scanner_env = _get_scanner_env(settings)

        _update_scan(scan_id, status="cloning")
        repo_name = _repo_name_from_url(repo_url)
        tmpdir = os.path.join(SCAN_REPO_BASE, f"{repo_name}_{scan_id}")
        os.makedirs(tmpdir, exist_ok=True)
        repo_dir = os.path.join(tmpdir, repo_name)

        clone_cmd = ["git", "clone", "--depth", "1"]
        if branch:
            clone_cmd.extend(["--branch", branch])
        clone_cmd.extend([repo_url, repo_dir])

        clone_result = subprocess.run(clone_cmd, capture_output=True, text=True, timeout=120, env=scanner_env)
        if clone_result.returncode != 0:
            _update_scan(scan_id, status="failed",
                         error=f"Git clone failed: {clone_result.stderr[:2000]}",
                         completed_at=datetime.now(timezone.utc))
            return

        _update_scan(scan_id, status="running")
        output_file = os.path.join(tmpdir, "output.json")

        custom_configs = []
        if tool == "semgrep":
            from db import semgrep_rules_col
            custom_rules = list(semgrep_rules_col.find())
            if custom_rules:
                rules_dir = os.path.join(tmpdir, "custom_rules")
                os.makedirs(rules_dir, exist_ok=True)
                for i, rule in enumerate(custom_rules):
                    rule_path = os.path.join(rules_dir, f"rule_{i}.yaml")
                    with open(rule_path, "w") as rf:
                        rf.write(rule["content"])
                    custom_configs.append(rule_path)

        success, stdout, stderr = _run_scanner_command(tool, repo_dir, output_file, env=scanner_env, custom_configs=custom_configs)

        log_text = (stdout + "\n" + stderr)[:10000]
        _update_scan(scan_id, log=log_text)

        if not success:
            error_lines = [l for l in stderr.strip().splitlines()
                           if not l.startswith("METRICS:") and "semgrep.dev/docs/metrics" not in l
                           and l.strip()]
            error_detail = "\n".join(error_lines) or stdout.strip() or "No output (scanner may have crashed)"
            _update_scan(scan_id, status="failed",
                         error=f"Scanner failed: {error_detail[:2000]}",
                         completed_at=datetime.now(timezone.utc))
            return

        _update_scan(scan_id, status="parsing")

        output_format = scanner_info["output_format"]
        findings = []

        if os.path.exists(output_file) and os.path.getsize(output_file) > 0:
            with open(output_file, "r") as f:
                raw_text = f.read()

            if output_format == "sarif":
                try:
                    sarif_data = json.loads(raw_text)
                    findings = parse_sarif(sarif_data)
                except json.JSONDecodeError as e:
                    _update_scan(scan_id, status="failed",
                                 error=f"Failed to parse SARIF output: {str(e)}",
                                 completed_at=datetime.now(timezone.utc))
                    return
            elif output_format == "trufflehog":
                fmt, parsed = detect_format(raw_text)
                if fmt == "trufflehog":
                    findings = parse_trufflehog(parsed)
                else:
                    _update_scan(scan_id, status="failed",
                                 error=f"Failed to parse TruffleHog output (detected as '{fmt}')",
                                 completed_at=datetime.now(timezone.utc))
                    return
            elif output_format == "gitleaks":
                fmt, parsed = detect_format(raw_text)
                if fmt == "gitleaks":
                    findings = parse_gitleaks(parsed)
                else:
                    _update_scan(scan_id, status="failed",
                                 error=f"Failed to parse gitleaks output (detected as '{fmt}')",
                                 completed_at=datetime.now(timezone.utc))
                    return

        inserted, duplicates = insert_findings_for_project(project_id, findings, output_format, f"scan-{tool}")

        completed = datetime.now(timezone.utc)
        started = scan.get("started_at")
        if started and started.tzinfo is None:
            started = started.replace(tzinfo=timezone.utc)
        duration = (completed - started).total_seconds() if started else 0

        _update_scan(scan_id,
                     status="completed",
                     completed_at=completed,
                     duration_seconds=round(duration, 1),
                     findings_count=inserted,
                     duplicates_count=duplicates)

    except subprocess.TimeoutExpired:
        _update_scan(scan_id, status="failed",
                     error="Scanner timed out (30 min limit)",
                     completed_at=datetime.now(timezone.utc))
    except Exception as e:
        _update_scan(scan_id, status="failed",
                     error=str(e)[:2000],
                     completed_at=datetime.now(timezone.utc))
    finally:
        if tmpdir and os.path.exists(tmpdir):
            shutil.rmtree(tmpdir, ignore_errors=True)
