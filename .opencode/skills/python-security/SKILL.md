---
name: python-security
description: Python security best practices for the legacy CLI in cli/
---

## What I do

I enforce Python security best practices for the legacy CLI in `cli/`. Based on OpenSSF, Black Duck, and Ericsson guidelines.

## Security rules

### 1. Input validation

```python
# Good
def process_file(path: str) -> None:
    if not path.startswith('/'):
        raise ValueError("Path must be absolute")
    # ...

# Bad
def process_file(path: str) -> None:
    # No validation
    os.system(f"cat {path}")
```

### 2. Command injection prevention

```python
# Good
import subprocess
subprocess.run(['ls', '-la', path], capture_output=True)

# Bad
os.system(f"ls -la {path}")
```

### 3. Secret handling

```python
# Good
import os
api_key = os.environ.get('API_KEY')
if not api_key:
    raise ValueError("API_KEY not set")

# Bad
api_key = "hardcoded-secret"
```

### 4. File permissions

```python
# Good
import os
os.chmod(path, 0o600)  # Owner read/write only

# Bad
os.chmod(path, 0o777)  # World readable/writable
```

### 5. Temporary files

```python
# Good
import tempfile
with tempfile.NamedTemporaryFile() as tmp:
    # tmp is automatically deleted
    pass

# Bad
tmp_path = "/tmp/myfile"
# File may persist after crash
```

## Linting rules

```bash
# Run ruff
ruff check xbin/

# Run black
black --check xbin/

# Run bandit (security linter)
bandit -r xbin/
```

## Common issues

### SQL injection
```python
# Good
cursor.execute("SELECT * FROM users WHERE id = ?", (user_id,))

# Bad
cursor.execute(f"SELECT * FROM users WHERE id = {user_id}")
```

### Path traversal
```python
# Good
from pathlib import Path
safe_path = Path(base_dir) / user_input
if not safe_path.resolve().startswith(base_dir):
    raise ValueError("Path traversal detected")

# Bad
unsafe_path = os.path.join(base_dir, user_input)
```

### Unsafe deserialization
```python
# Good
import json
data = json.loads(safe_input)

# Bad
import pickle
data = pickle.loads(unsafe_input)
```

## Files to audit

- `cli/xbin/*.py`: Main CLI code
- `cli/xbin/runtimes/*.py`: Runtime detection
- `cli/tests/*.py`: Test code
- `cli/pyproject.toml`: Dependencies

## Testing

```bash
# Run tests
cd cli && python -m pytest

# Run security tests
bandit -r xbin/

# Check dependencies
pip-audit
```
