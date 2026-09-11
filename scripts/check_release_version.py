#!/usr/bin/env python3
"""Check a release tag against Cargo.toml and expose the package version."""

from __future__ import annotations

import os
import re
from pathlib import Path


def main() -> None:
    """Read the Cargo package version and reject a mismatched tag."""
    cargo = Path(__file__).resolve().parents[1] / "Cargo.toml"
    package = re.search(r"^\[package\]\n(?P<body>.*?)(?=^\[|\Z)", cargo.read_text(), re.MULTILINE | re.DOTALL)
    version = re.search(r'^version\s*=\s*"([^"]+)"', package["body"], re.MULTILINE) if package else None
    if version is None:
        raise SystemExit("cannot read [package].version from Cargo.toml")
    value = version[1]
    if os.environ.get("GITHUB_REF_TYPE") == "tag" and os.environ.get("GITHUB_REF_NAME") != f"v{value}":
        raise SystemExit(f"tag {os.environ.get('GITHUB_REF_NAME')!r} does not match v{value}")
    print(value)


if __name__ == "__main__":
    main()
