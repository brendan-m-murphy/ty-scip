#!/usr/bin/env python3
"""Validate and smoke-test one built ty-scip wheel."""

from __future__ import annotations

import argparse
from email.parser import BytesParser
import glob
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tempfile
import venv
from zipfile import ZipFile


def one_member(names: list[str], suffix: str) -> str:
    """Return the sole archive member ending with suffix."""
    matches = [name for name in names if name.endswith(suffix)]
    if len(matches) != 1:
        raise SystemExit(f"expected one *{suffix}, found {matches}")
    return matches[0]


def inspect(wheel: Path) -> str:
    """Check wheel structure and metadata, returning its declared version."""
    with ZipFile(wheel) as archive:
        names = archive.namelist()
        unsafe = [
            name
            for name in names
            if PurePosixPath(name).is_absolute() or ".." in PurePosixPath(name).parts
        ]
        if unsafe:
            raise SystemExit(f"unsafe wheel members: {unsafe}")

        one_member(names, ".dist-info/licenses/LICENSE")
        one_member(names, ".dist-info/licenses/THIRD_PARTY_NOTICES")
        binary = [
            name
            for name in names
            if name.endswith(".data/scripts/ty-scip")
            or name.endswith(".data/scripts/ty-scip.exe")
        ]
        if len(binary) != 1:
            raise SystemExit(f"expected one ty-scip executable, found {binary}")
        if any(".dist-info/sboms/" in name for name in names):
            raise SystemExit("wheel contains an SBOM despite the release metadata policy")

        metadata_name = one_member(names, ".dist-info/METADATA")
        metadata = BytesParser().parsebytes(archive.read(metadata_name))
        if metadata.get("License-Expression") != "MIT":
            raise SystemExit("wheel metadata lacks `License-Expression: MIT`")
        if sorted(metadata.get_all("License-File", [])) != [
            "LICENSE",
            "THIRD_PARTY_NOTICES",
        ]:
            raise SystemExit("wheel metadata has incorrect License-File entries")

        local_path = re.compile(
            rb"file://|path\+file://|[A-Za-z]:\\+|/(?:Users|home|private/tmp|tmp|workspace)/"
        )
        for name in names:
            if name == binary[0]:
                continue
            data = archive.read(name)
            if local_path.search(data):
                raise SystemExit(f"wheel metadata exposes a local path in {name}")
        return metadata["Version"]


def smoke_test(wheel: Path, version: str) -> None:
    """Install the wheel into an isolated environment and run its executable."""
    with tempfile.TemporaryDirectory(prefix="ty-scip-wheel-") as directory:
        environment = Path(directory)
        venv.EnvBuilder(with_pip=True).create(environment)
        scripts = environment / ("Scripts" if os.name == "nt" else "bin")
        python = scripts / ("python.exe" if os.name == "nt" else "python")
        subprocess.run(
            [
                python,
                "-m",
                "pip",
                "install",
                "--no-index",
                "--no-deps",
                "--no-cache-dir",
                "--disable-pip-version-check",
                wheel.resolve(),
            ],
            check=True,
        )
        executables = [
            path for path in scripts.iterdir() if path.name in {"ty-scip", "ty-scip.exe"}
        ]
        if len(executables) != 1:
            raise SystemExit(f"expected one installed ty-scip executable, found {executables}")
        output = subprocess.run(
            [executables[0], "--version"],
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        ).stdout.strip()
        if output != f"ty-scip {version}":
            raise SystemExit(f"unexpected version output: {output!r}")


def main() -> None:
    """Parse arguments, validate the wheel, and run the smoke test."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("wheel", help="wheel path or a pattern matching exactly one wheel")
    args = parser.parse_args()
    wheels = [Path(path) for path in glob.glob(args.wheel)]
    if len(wheels) != 1:
        raise SystemExit(f"expected one wheel, found {wheels}")
    version = inspect(wheels[0])
    smoke_test(wheels[0], version)
    print(f"validated {wheels[0]}")


if __name__ == "__main__":
    main()
