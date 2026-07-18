import os

from cryptography.fernet import Fernet, InvalidToken
from pymongo import MongoClient

# Key must be stable across all Gunicorn workers and container restarts.
# Priority: ENCRYPTION_KEY env var > key stored in MongoDB.

_mongo_uri = os.environ.get("MONGO_URI", "mongodb://localhost:27017/sarif_manager")
_key = os.environ.get("ENCRYPTION_KEY")

if not _key:
    _client = MongoClient(_mongo_uri)
    _db = _client.get_default_database()
    _meta = _db["_meta"]

    doc = _meta.find_one({"_id": "encryption_key"})
    if doc:
        _key = doc["value"]
    else:
        _key = Fernet.generate_key().decode()
        _meta.insert_one({"_id": "encryption_key", "value": _key})
    _client.close()

_fernet = Fernet(_key.encode() if isinstance(_key, str) else _key)


def encrypt(value):
    """Encrypt a plaintext string, returns base64-encoded ciphertext."""
    return _fernet.encrypt(value.encode()).decode()


def decrypt(value):
    """Decrypt a ciphertext string. Returns None if decryption fails."""
    try:
        return _fernet.decrypt(value.encode()).decode()
    except (InvalidToken, Exception):
        return None
