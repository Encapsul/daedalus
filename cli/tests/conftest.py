"""Pytest configuration for xbin tests."""

import sys
from pathlib import Path

# Add cli/ to path so xbin modules can be imported
sys.path.insert(0, str(Path(__file__).parent.parent))
