#!/usr/bin/env python3
"""Run the frozen OpenGHG release gate for a candidate ``ty-scip`` binary.

The gate builds the patched ``scip-python`` reference, indexes the same frozen
checkout with both indexers, compares scheme-independent first-party targets,
checks the candidate's decoded structure, and exercises a fresh ``scip-cli``
database.  Only the Python standard library is used by this orchestrator; the
required external tools are documented in ``docs/release-gate.md``.
"""

from __future__ import annotations

import argparse
import filecmp
import hashlib
import json
import os
import re
import shlex
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

OPENGHG_URL = "https://github.com/openghg/openghg.git"
OPENGHG_REVISION = "a4d352f5972e84774ad36f238af9fe2eec2c4d2e"
SCIP_PYTHON_URL = "https://github.com/sourcegraph/scip-python.git"
SCIP_PYTHON_REVISION = "468008597e371ed4abac73ea2a14a08bbd16c7d1"
RUFF_REVISION = "12132db2885084f4b87aafc5908e336e7b5e8fbd"
EXPECTED_DOCUMENTS = 281
EXPECTED_DIFFERENTIAL_SHA256 = "55b8e718920ea47b8fa710d926b6d48894338ec1a5e6ddc98f2b2587915e6c72"

# These are genuine property/inherited-receiver disagreements, not regressions.
KNOWN_DIVERGENCES = frozenset(
    {
        ("openghg/storage/_store.py", (292, 20, 28)),
        ("openghg/storage/_store.py", (296, 13, 21)),
        ("openghg/storage/_zarr_store.py", (82, 20, 35)),
        ("openghg/storage/_zarr_store.py", (368, 20, 28)),
        ("openghg/storage/_zarr_store.py", (372, 13, 21)),
        ("openghg/storage/_zarr_store.py", (389, 22, 30)),
    }
)


class GateError(RuntimeError):
    """Report one actionable release-gate failure."""


def _command(name: str) -> str:
    """Return an external command path or fail with an actionable message."""
    path = shutil.which(name)
    if path is None:
        raise GateError(f"required command is not on PATH: {name}")
    return path


def _run(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    check: bool = True,
) -> tuple[subprocess.CompletedProcess[str], float]:
    """Run a command, echo its output, and return the result and duration."""
    print(f"+ {shlex.join(command)}", flush=True)
    started = time.perf_counter()
    result = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    duration = time.perf_counter() - started
    if result.stdout:
        print(result.stdout, end="" if result.stdout.endswith("\n") else "\n")
    if result.stderr:
        print(result.stderr, end="" if result.stderr.endswith("\n") else "\n", file=sys.stderr)
    if check and result.returncode:
        raise GateError(f"command failed with exit code {result.returncode}: {shlex.join(command)}")
    return result, duration


def _run_to_file(
    command: list[str], output: Path, *, cwd: Path | None = None, env: dict[str, str] | None = None
) -> float:
    """Run a command with stdout written directly to a potentially large file."""
    print(f"+ {shlex.join(command)} > {output}", flush=True)
    started = time.perf_counter()
    with output.open("w", encoding="utf-8") as stream:
        result = subprocess.run(
            command,
            cwd=cwd,
            env=env,
            text=True,
            stdout=stream,
            stderr=subprocess.PIPE,
            check=False,
        )
    if result.stderr:
        print(result.stderr, end="" if result.stderr.endswith("\n") else "\n", file=sys.stderr)
    if result.returncode:
        raise GateError(f"command failed with exit code {result.returncode}: {shlex.join(command)}")
    return time.perf_counter() - started


def _clone_at(url: str, destination: Path, revision: str, *, ref: str | None = None) -> None:
    """Clone a repository and detach at an exact verified revision."""
    _run([_command("git"), "clone", "--filter=blob:none", "--no-checkout", url, str(destination)])
    if ref is not None:
        _run([_command("git"), "-C", str(destination), "fetch", "--depth=1", "origin", ref])
    _run([_command("git"), "-C", str(destination), "checkout", "--detach", revision])
    actual, _ = _run([_command("git"), "-C", str(destination), "rev-parse", "HEAD"])
    if actual.stdout.strip() != revision:
        raise GateError(f"checkout mismatch for {destination}: {actual.stdout.strip()}")


def _symbol_key(path: str, symbol: str) -> tuple[str, str]:
    """Qualify SCIP document-local symbols while leaving global symbols portable."""
    return (path if symbol.startswith("local ") else "", symbol)


def _definitions(
    index: dict[str, Any],
) -> dict[tuple[str, str], frozenset[tuple[str, tuple[int, ...]]]]:
    """Map document-qualified symbols to their definition locations."""
    definitions: dict[tuple[str, str], set[tuple[str, tuple[int, ...]]]] = {}
    for document in index.get("documents", []):
        path = document["relative_path"]
        for occurrence in document.get("occurrences", []):
            if occurrence.get("symbol_roles", 0) & 1:
                key = _symbol_key(path, occurrence["symbol"])
                definitions.setdefault(key, set()).add((path, tuple(occurrence["range"])))
    return {symbol: frozenset(locations) for symbol, locations in definitions.items()}


def _projection(
    index: dict[str, Any],
) -> dict[tuple[str, tuple[int, ...]], frozenset[tuple[str, tuple[int, ...]]]]:
    """Project reference ranges to definition locations without using symbol schemes."""
    definitions = _definitions(index)

    projection: dict[tuple[str, tuple[int, ...]], set[tuple[str, tuple[int, ...]]]] = {}
    for document in index.get("documents", []):
        path = document["relative_path"]
        for occurrence in document.get("occurrences", []):
            if occurrence.get("symbol_roles", 0) & 1:
                continue
            targets = definitions.get(_symbol_key(path, occurrence["symbol"]))
            if targets:
                projection.setdefault((path, tuple(occurrence["range"])), set()).update(targets)
    return {source: frozenset(targets) for source, targets in projection.items()}


def _relationships(
    index: dict[str, Any],
) -> frozenset[
    tuple[
        tuple[str, tuple[int, ...]],
        tuple[str, tuple[int, ...]],
        tuple[str, ...],
    ]
]:
    """Project SCIP relationships to definition locations and semantic flags."""
    definitions = _definitions(index)
    projected = set()
    flag_names = ("is_definition", "is_implementation", "is_reference", "is_type_definition")
    for document in index.get("documents", []):
        path = document["relative_path"]
        for information in document.get("symbols", []):
            sources = definitions.get(_symbol_key(path, information["symbol"]), ())
            for relationship in information.get("relationships", []):
                targets = definitions.get(_symbol_key(path, relationship.get("symbol", "")), ())
                flags = tuple(name for name in flag_names if relationship.get(name))
                projected.update((source, target, flags) for source in sources for target in targets)
    return frozenset(projected)


def _location(value: tuple[str, tuple[int, ...]]) -> dict[str, Any]:
    """Return a JSON-friendly source or target location."""
    return {"path": value[0], "range": list(value[1])}


def _relationship(
    value: tuple[
        tuple[str, tuple[int, ...]],
        tuple[str, tuple[int, ...]],
        tuple[str, ...],
    ],
) -> dict[str, Any]:
    """Return one JSON-friendly projected relationship."""
    return {"source": _location(value[0]), "target": _location(value[1]), "flags": value[2]}


def _differential(
    candidate: dict[tuple[str, tuple[int, ...]], frozenset[tuple[str, tuple[int, ...]]]],
    reference: dict[tuple[str, tuple[int, ...]], frozenset[tuple[str, tuple[int, ...]]]],
    candidate_relationships: frozenset,
    reference_relationships: frozenset,
) -> tuple[dict[str, Any], frozenset[tuple[str, tuple[int, ...]]]]:
    """Build a deterministic target report and return sources missing reference targets."""
    candidate_sources = candidate.keys()
    reference_sources = reference.keys()
    shared = candidate_sources & reference_sources
    target_differences = []
    missing_reference_targets = set()
    for source in sorted(shared):
        candidate_only = sorted(candidate[source] - reference[source])
        reference_only = sorted(reference[source] - candidate[source])
        if not candidate_only and not reference_only:
            continue
        if reference_only:
            missing_reference_targets.add(source)
        target_differences.append(
            {
                "source": _location(source),
                "candidate_only_targets": [_location(target) for target in candidate_only],
                "reference_only_targets": [_location(target) for target in reference_only],
            }
        )
    report = {
        "candidate_projected_references": len(candidate),
        "reference_projected_references": len(reference),
        "shared_projected_references": len(shared),
        "candidate_only_sources": [
            _location(source) for source in sorted(candidate_sources - reference_sources)
        ],
        "reference_only_sources": [
            _location(source) for source in sorted(reference_sources - candidate_sources)
        ],
        "target_differences": target_differences,
        "candidate_only_relationships": [
            _relationship(item) for item in sorted(candidate_relationships - reference_relationships)
        ],
        "reference_only_relationships": [
            _relationship(item) for item in sorted(reference_relationships - candidate_relationships)
        ],
    }
    return report, frozenset(missing_reference_targets)


def _integrity(index: dict[str, Any]) -> dict[str, int]:  # noqa: C901, PLR0912
    """Check structural invariants that are stronger than SCIP 0.8 relationship lint."""
    documents = index.get("documents", [])
    if len(documents) != EXPECTED_DOCUMENTS:
        raise GateError(f"candidate contains {len(documents)} documents; expected {EXPECTED_DOCUMENTS}")

    all_information = {item["symbol"] for item in index.get("external_symbols", [])}
    definition_symbols = {
        occurrence["symbol"]
        for document in documents
        for occurrence in document.get("occurrences", [])
        if occurrence.get("symbol_roles", 0) & 1
    }
    definitions = references = 0
    missing_definitions: list[str] = []
    missing_relationships: list[str] = []
    missing_relationship_definitions: list[str] = []
    missing_stdlib: list[str] = []
    for document in documents:
        path = document["relative_path"]
        local_information = {item["symbol"] for item in document.get("symbols", [])}
        all_information.update(local_information)
        for occurrence in document.get("occurrences", []):
            symbol = occurrence["symbol"]
            if occurrence.get("symbol_roles", 0) & 1:
                definitions += 1
                if symbol not in local_information:
                    missing_definitions.append(f"{path}:{occurrence['range']} {symbol}")
            else:
                references += 1
            if symbol.startswith("ty-scip python python-stdlib ") and symbol not in all_information:
                missing_stdlib.append(f"{path}:{occurrence['range']} {symbol}")

    for document in documents:
        for information in document.get("symbols", []):
            for relationship in information.get("relationships", []):
                target = relationship.get("symbol", "")
                if target and target not in all_information:
                    missing_relationships.append(target)
                if target.startswith("ty-scip python openghg ") and target not in definition_symbols:
                    missing_relationship_definitions.append(target)

    problems = {
        "definitions without same-document symbol information": missing_definitions,
        "relationships without target symbol information": missing_relationships,
        "first-party relationships without target definitions": missing_relationship_definitions,
        "stdlib occurrences without external symbol information": missing_stdlib,
    }
    for label, values in problems.items():
        if values:
            raise GateError(f"{label}: {len(values)} (first: {values[0]})")
    return {"documents": len(documents), "definitions": definitions, "references": references}


def _load(path: Path) -> dict[str, Any]:
    """Load one JSON-printed SCIP index."""
    with path.open(encoding="utf-8") as stream:
        value = json.load(stream)
    if not isinstance(value, dict):
        raise GateError(f"expected a JSON object in {path}")
    return value


def _scip_cli_python(scip_cli: str) -> str:
    """Find the interpreter belonging to a virtual-environment scip-cli script."""
    directory = Path(scip_cli).resolve().parent
    candidate = directory / ("python.exe" if os.name == "nt" else "python")
    if not candidate.is_file():
        raise GateError(f"cannot find scip-cli's Python interpreter beside {scip_cli}")
    return str(candidate)


def _convert_candidate(
    scip_cli_python: str, root: Path, index: Path, env: dict[str, str]
) -> tuple[Path, str, float]:
    """Convert a candidate index into the cache path used by actual scip-cli queries."""
    code = (
        "from pathlib import Path; import sys; "
        "from scip_cli.cache import get_cache_dir; "
        "from scip_cli.indexing.convert import convert_scip_to_db, resolve_scip_binary; "
        "db=get_cache_dir(Path(sys.argv[1]))/'index.db'; "
        "convert_scip_to_db(Path(sys.argv[2]), db); "
        "print(db); print(resolve_scip_binary())"
    )
    result, duration = _run([scip_cli_python, "-c", code, str(root), str(index)], env=env)
    lines = [line.strip() for line in result.stdout.splitlines() if line.strip()]
    try:
        database_text, scip = lines[-2:]
    except ValueError:
        raise GateError("scip-cli conversion did not report its database and SCIP binary paths")
    database = Path(database_text)
    if not database.is_file():
        raise GateError(f"scip-cli did not create {database}")
    return database, scip, duration


def _require_output(
    label: str, output: str, required: tuple[str, ...], forbidden: tuple[str, ...] = ()
) -> None:
    """Require exact evidence in one consumer query result."""
    missing = [text for text in required if text not in output]
    present = [text for text in forbidden if text in output]
    if missing or present:
        raise GateError(f"{label} failed; missing={missing!r}, forbidden present={present!r}")


def _consumer_gate(scip_cli: str, root: Path, env: dict[str, str]) -> None:
    """Run the exact search/member/reference/dependency acceptance probes."""
    probes = [
        (
            "search",
            ["search", "ModelScenario", "--limit", "20"],
            ("openghg/analyse/_scenario.py:96 class ModelScenario",),
            (),
        ),
        (
            "members",
            ["members", "ModelScenario", "--path", "openghg/analyse/_scenario.py", "--limit", "100"],
            ("calc_modelled_obs", "_calc_modelled_obs_HiTRes", "footprints_data_merge"),
            (),
        ),
        (
            "numba refs",
            ["refs", "fp_x_flux_time_resolved_numba", "--limit", "50"],
            (
                "openghg/analyse/__init__.py:2",
                "tests/analyse/test_fp_x_flux.py:11",
                "tests/analyse/test_fp_x_flux.py:151",
            ),
            ("openghg/analyse/_scenario.py",),
        ),
        (
            "legacy refs",
            ["refs", "fp_x_flux_time_resolved", "--limit", "50"],
            ("openghg/analyse/_scenario.py:57", "openghg/analyse/_scenario.py:1394"),
            (),
        ),
        (
            "dependencies",
            ["deps", "openghg/analyse/_scenario.py", "--limit", "100"],
            ("fp_x_flux_time_resolved",),
            (),
        ),
        (
            "reverse dependencies",
            ["rdeps", "openghg/analyse/_fp_x_flux.py", "--limit", "100"],
            ("openghg/analyse/__init__.py", "tests/analyse/test_fp_x_flux.py"),
            (),
        ),
    ]
    for label, arguments, required, forbidden in probes:
        result, _ = _run([scip_cli, *arguments], cwd=root, env=env)
        _require_output(label, result.stdout + result.stderr, required, forbidden)


def _database_metrics(database: Path) -> dict[str, int]:
    """Count stable scip-cli database entities without imposing coverage thresholds."""
    tables = ("documents", "chunks", "mentions", "defn_enclosing_ranges", "global_symbols")
    with sqlite3.connect(database) as connection:
        return {table: connection.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0] for table in tables}


def _validate_database_metrics(metrics: dict[str, int]) -> None:
    """Reject an empty or incomplete scip-cli conversion."""
    if metrics.get("documents") != EXPECTED_DOCUMENTS:
        raise GateError(
            f"scip-cli conversion contains {metrics.get('documents', 0)} documents; "
            f"expected {EXPECTED_DOCUMENTS}"
        )
    empty = [
        name
        for name in ("chunks", "mentions", "defn_enclosing_ranges", "global_symbols")
        if not metrics.get(name)
    ]
    if empty:
        raise GateError(f"scip-cli conversion produced empty tables: {empty}")


def _sha256(path: Path) -> str:
    """Return the SHA-256 digest of a file."""
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _summary(text: str) -> dict[str, int]:
    """Parse ty-scip's stable index summary counters."""
    match = re.search(
        r"indexed (?P<documents>\d+) files: (?P<definitions>\d+) definitions, "
        r"(?P<references>\d+) references; (?P<unresolved>\d+) unresolved, "
        r"(?P<ambiguous>\d+) ambiguous, (?P<external>\d+) external, "
        r"(?P<skipped>\d+) skipped",
        text,
    )
    if match is None:
        raise GateError("candidate did not emit the expected index summary")
    return {name: int(value) for name, value in match.groupdict().items()}


def _parse_args() -> argparse.Namespace:
    """Parse the intentionally small release-gate command line."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ty-scip", required=True, help="candidate ty-scip executable")
    parser.add_argument("--work-dir", type=Path, help="new directory for retained gate artifacts")
    parser.add_argument("--keep", action="store_true", help="retain an automatically-created work directory")
    return parser.parse_args()


def _main() -> int:  # noqa: C901, PLR0912, PLR0915
    """Run the complete gate and report actionable failures."""
    args = _parse_args()
    ty_scip = Path(shutil.which(args.ty_scip) or args.ty_scip).resolve()
    if not ty_scip.is_file():
        raise GateError(f"candidate ty-scip binary does not exist: {ty_scip}")
    candidate_version, _ = _run([str(ty_scip), "--version"])
    version = candidate_version.stdout.strip()
    if not re.fullmatch(r"ty-scip \d+\.\d+\.\d+", version):
        raise GateError(f"unexpected candidate version output: {version!r}")
    _command("git")
    npm, node, scip_cli = (_command(name) for name in ("npm", "node", "scip-cli"))

    if args.work_dir:
        work = args.work_dir.resolve()
        if work.exists() and any(work.iterdir()):
            raise GateError(f"work directory must be absent or empty: {work}")
        work.mkdir(parents=True, exist_ok=True)
        keep = True
    else:
        work = Path(tempfile.mkdtemp(prefix="ty-scip-openghg-gate-"))
        keep = args.keep
    print(f"work directory: {work}")

    try:
        openghg = work / "openghg"
        scip_python = work / "scip-python"
        _clone_at(OPENGHG_URL, openghg, OPENGHG_REVISION)
        _clone_at(
            SCIP_PYTHON_URL,
            scip_python,
            SCIP_PYTHON_REVISION,
            ref="pull/226/head",
        )
        _run([npm, "ci"], cwd=scip_python)
        _run([npm, "run", "build"], cwd=scip_python / "packages" / "pyright-scip")

        candidate_a = work / "ty-scip-a.scip"
        candidate_b = work / "ty-scip-b.scip"
        candidate_command = [
            str(ty_scip),
            "index",
            "--cwd",
            str(openghg),
            "--project-name",
            "openghg",
            "--project-version",
            OPENGHG_REVISION,
            "--output",
        ]
        candidate_results = []
        for output in (candidate_a, candidate_b):
            result, duration = _run([*candidate_command, str(output), "."], cwd=openghg)
            candidate_results.append(
                {
                    "seconds": duration,
                    "summary": _summary(result.stderr),
                }
            )
        if not filecmp.cmp(candidate_a, candidate_b, shallow=False):
            raise GateError("two candidate runs produced different SCIP bytes")
        if candidate_results[0]["summary"] != candidate_results[1]["summary"]:
            raise GateError("two candidate runs reported different summary counters")

        empty_environment = work / "empty-environment.json"
        empty_environment.write_text("[]\n", encoding="utf-8")
        reference = work / "scip-python.scip"
        _, reference_seconds = _run(
            [
                node,
                str(scip_python / "packages" / "pyright-scip" / "index.js"),
                "index",
                "--cwd",
                str(openghg),
                "--project-name",
                "openghg",
                "--project-version",
                OPENGHG_REVISION,
                "--environment",
                str(empty_environment),
                "--output",
                str(reference),
                "--quiet",
            ],
            cwd=openghg,
        )

        isolated_home = work / "consumer-home"
        isolated_home.mkdir()
        consumer_env = os.environ.copy()
        consumer_env.update(
            {
                "HOME": str(isolated_home),
                "USERPROFILE": str(isolated_home),
                "PATH": os.pathsep.join((str(Path(scip_cli).resolve().parent), "/usr/bin", "/bin")),
            }
        )
        cli_version, _ = _run([scip_cli, "--version"], env=consumer_env)
        if not re.search(r"\b2\.7\.0\b", cli_version.stdout + cli_version.stderr):
            raise GateError("release gate requires scip-cli 2.7.0")
        database, scip, conversion_seconds = _convert_candidate(
            _scip_cli_python(scip_cli), openghg, candidate_a, consumer_env
        )
        database_metrics = _database_metrics(database)
        _validate_database_metrics(database_metrics)
        scip_version, _ = _run([scip, "--version"], env=consumer_env)
        if not re.search(r"\bv?0\.8\.1\b", scip_version.stdout + scip_version.stderr):
            raise GateError("scip-cli must supply SCIP 0.8.1 for the release gate")

        candidate_json = work / "ty-scip.json"
        reference_json = work / "scip-python.json"
        candidate_print_seconds = _run_to_file(
            [scip, "print", "--json", str(candidate_a)], candidate_json, env=consumer_env
        )
        reference_print_seconds = _run_to_file(
            [scip, "print", "--json", str(reference)], reference_json, env=consumer_env
        )
        candidate_data = _load(candidate_json)
        integrity = _integrity(candidate_data)
        candidate_projection = _projection(candidate_data)
        candidate_relationships = _relationships(candidate_data)
        del candidate_data
        reference_data = _load(reference_json)
        reference_projection = _projection(reference_data)
        reference_relationships = _relationships(reference_data)
        del reference_data
        differential, missing_reference_targets = _differential(
            candidate_projection,
            reference_projection,
            candidate_relationships,
            reference_relationships,
        )
        differential_path = work / "differential.json"
        differential_path.write_text(
            json.dumps(differential, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        if missing_reference_targets != KNOWN_DIVERGENCES:
            added = sorted(missing_reference_targets - KNOWN_DIVERGENCES)
            missing = sorted(KNOWN_DIVERGENCES - missing_reference_targets)
            raise GateError(f"differential mismatch; unexpected={added!r}, missing known={missing!r}")
        differential_sha256 = _sha256(differential_path)
        if differential_sha256 != EXPECTED_DIFFERENTIAL_SHA256:
            raise GateError(
                "differential report changed; review it and deliberately update "
                f"EXPECTED_DIFFERENTIAL_SHA256 (got {differential_sha256})"
            )

        lint, lint_seconds = _run([scip, "lint", str(candidate_a)], env=consumer_env, check=False)
        _consumer_gate(scip_cli, openghg, consumer_env)

        metrics = {
            "revisions": {
                "openghg": OPENGHG_REVISION,
                "ruff": RUFF_REVISION,
                "scip_python": SCIP_PYTHON_REVISION,
            },
            "candidate": {
                "binary_sha256": _sha256(ty_scip),
                "bytes": candidate_a.stat().st_size,
                "sha256": _sha256(candidate_a),
                "version": version,
                "runs": candidate_results,
                **integrity,
                "projected_references": len(candidate_projection),
            },
            "reference": {
                "bytes": reference.stat().st_size,
                "seconds": reference_seconds,
                "projected_references": len(reference_projection),
            },
            "differential": {
                "sha256": differential_sha256,
                "candidate_only_sources": len(differential["candidate_only_sources"]),
                "reference_only_sources": len(differential["reference_only_sources"]),
                "shared_projected_references": differential["shared_projected_references"],
                "target_differences": len(differential["target_differences"]),
                "known_missing_reference_targets": len(missing_reference_targets),
                "candidate_only_relationships": len(differential["candidate_only_relationships"]),
                "reference_only_relationships": len(differential["reference_only_relationships"]),
            },
            "consumer_database": database_metrics,
            "timings_seconds": {
                "conversion": conversion_seconds,
                "candidate_json_print": candidate_print_seconds,
                "reference_json_print": reference_print_seconds,
                "lint": lint_seconds,
            },
            "scip_0_8_lint": {
                "exit_code": lint.returncode,
                "output_lines": len((lint.stdout + lint.stderr).splitlines()),
            },
        }
        (work / "metrics.json").write_text(
            json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print("release gate passed")
        print(json.dumps(metrics, indent=2, sort_keys=True))
        return 0
    finally:
        if keep:
            print(f"kept work directory: {work}")
        else:
            shutil.rmtree(work)


if __name__ == "__main__":
    try:
        raise SystemExit(_main())
    except GateError as error:
        print(f"release gate failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
