#!/usr/bin/env python3
"""Generate or check the locked third-party license bundle."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "THIRD_PARTY_NOTICES"
CARGO_ABOUT_VERSION = "0.9.2"


def run(*args: str) -> str:
    """Run a command from the repository root and return stdout."""
    return subprocess.run(
        args,
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    ).stdout


def cargo_about() -> str:
    """Return a suitable cargo-about executable or fail with install help."""
    executable = os.environ.get("CARGO_ABOUT") or shutil.which("cargo-about")
    if executable is None:
        raise SystemExit(
            f"cargo-about {CARGO_ABOUT_VERSION} is required; run "
            f"`cargo install cargo-about --locked --version {CARGO_ABOUT_VERSION} "
            "--features cli`"
        )
    version = run(executable, "--version").strip()
    if version != f"cargo-about {CARGO_ABOUT_VERSION}":
        raise SystemExit(
            f"expected cargo-about {CARGO_ABOUT_VERSION}, found {version!r}"
        )
    return executable


def package_url(package: dict[str, Any]) -> str:
    """Return the most useful public source URL for a Cargo package."""
    if package.get("repository"):
        return package["repository"]
    if (package.get("source") or "").startswith("registry+"):
        return f"https://crates.io/crates/{package['name']}/{package['version']}"
    return ""


def locate_ruff(metadata: dict[str, Any]) -> tuple[Path, str]:
    """Locate the pinned Ruff checkout and return its root and commit."""
    packages = [package for package in metadata["packages"] if package["name"] == "ty_vendored"]
    if len(packages) != 1:
        raise SystemExit(f"expected one ty_vendored package, found {len(packages)}")
    package = packages[0]
    source = package.get("source") or ""
    prefix = "git+https://github.com/astral-sh/ruff"
    if not source.startswith(prefix) or "#" not in source:
        raise SystemExit(f"unexpected ty_vendored source: {source!r}")
    return Path(package["manifest_path"]).parents[2], source.rsplit("#", 1)[1]


def generate() -> str:
    """Generate deterministic notices from Cargo metadata and upstream files."""
    about = cargo_about()
    with tempfile.TemporaryDirectory(prefix="ty-scip-licenses-") as directory:
        output = Path(directory) / "about.json"
        subprocess.run(
            [
                about,
                "generate",
                "--locked",
                "--offline",
                "--format",
                "json",
                "--output-file",
                output,
                "--fail",
            ],
            cwd=ROOT,
            check=True,
        )
        report = json.loads(output.read_text())

    metadata = json.loads(run("cargo", "metadata", "--locked", "--format-version", "1"))
    ruff_root, ruff_commit = locate_ruff(metadata)
    ruff_license = (ruff_root / "LICENSE").read_text()
    typeshed_root = ruff_root / "crates" / "ty_vendored" / "vendor" / "typeshed"
    typeshed_license = (typeshed_root / "LICENSE").read_text()
    typeshed_commit = (typeshed_root / "source_commit.txt").read_text().strip()
    lock_hash = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()

    crates = [
        crate["package"]
        for crate in report["crates"]
        if crate["package"]["name"] != "ty-scip"
    ]
    crates.sort(key=lambda package: (package["name"].casefold(), package["version"]))

    groups: dict[tuple[str, str, str], dict[tuple[str, str], str]] = {}
    for license_ in report["licenses"]:
        users = {
            (entry["crate"]["name"], entry["crate"]["version"]): package_url(
                entry["crate"]
            )
            for entry in license_["used_by"]
            if entry["crate"]["name"] != "ty-scip"
        }
        if users:
            key = (license_["id"], license_["name"], license_["text"].strip())
            groups.setdefault(key, {}).update(users)

    lines = [
        "# Third-party notices",
        "",
        "This file records the licenses of the code linked into `ty-scip`. It is",
        "generated from the locked, cross-platform Cargo dependency graph, then",
        "augmented with the complete license notices shipped by the pinned Ruff/ty",
        "source and the typeshed data embedded by `ty_vendored`.",
        "",
        f"- Cargo.lock SHA-256: `{lock_hash}`",
        f"- Ruff/ty commit: `{ruff_commit}`",
        f"- Typeshed commit: `{typeshed_commit}`",
        f"- Generator: `cargo-about {CARGO_ABOUT_VERSION}` and `scripts/licenses.py`",
        "",
        "The source links below identify where corresponding source can be obtained,",
        "including for dependencies licensed under MPL-2.0.",
        "",
        "## Locked dependency inventory",
        "",
        "| Package | Cargo license expression | Source |",
        "| --- | --- | --- |",
    ]
    for package in crates:
        url = package_url(package)
        source = f"[source]({url})" if url else "not provided"
        lines.append(
            f"| `{package['name']} {package['version']}` | "
            f"`{package.get('license') or 'not provided'}` | {source} |"
        )

    lines.extend(["", "## Resolved license texts", ""])
    for index, ((identifier, name, text), users) in enumerate(
        sorted(groups.items(), key=lambda item: (item[0][0], item[0][2])), start=1
    ):
        lines.extend([f"### {identifier}: {name} ({index})", "", "Used by:", ""])
        for (crate_name, version), url in sorted(users.items()):
            label = f"`{crate_name} {version}`"
            lines.append(f"- [{label}]({url})" if url else f"- {label}")
        lines.extend(["", "```text", text, "```", ""])

    lines.extend(
        [
            "## Ruff and ty",
            "",
            f"Pinned from <https://github.com/astral-sh/ruff/tree/{ruff_commit}>.",
            "This upstream license includes Ruff's inherited-code notices.",
            "",
            "```text",
            ruff_license.rstrip(),
            "```",
            "",
            "## Typeshed",
            "",
            f"Pinned from <https://github.com/python/typeshed/tree/{typeshed_commit}> and",
            "embedded in the executable by `ty_vendored`.",
            "",
            "```text",
            typeshed_license.rstrip(),
            "```",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> None:
    """Update the notice file, or check that it is current."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if notices are stale")
    args = parser.parse_args()
    content = generate()
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_bytes() != content.encode():
            raise SystemExit("THIRD_PARTY_NOTICES is stale; run scripts/licenses.py")
        print("THIRD_PARTY_NOTICES is current")
    else:
        OUTPUT.write_bytes(content.encode())
        print(f"wrote {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        sys.exit(error.returncode)
