import os

from pymongo import MongoClient

client = MongoClient(os.environ.get("MONGO_URI", "mongodb://localhost:27017/sarif_manager"))
db = client.get_default_database()

projects_col = db["projects"]
findings_col = db["findings"]
scans_col = db["scans"]
settings_col = db["settings"]
semgrep_rules_col = db["semgrep_rules"]

# Ensure index for fast per-project duplicate lookups
findings_col.create_index([("project_id", 1), ("dedup_hash", 1)])
