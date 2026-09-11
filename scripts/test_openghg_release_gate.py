#!/usr/bin/env python3
"""Focused tests for the frozen OpenGHG release gate."""

from __future__ import annotations

import unittest

from openghg_release_gate import (
    EXPECTED_DOCUMENTS,
    GateError,
    _differential,
    _summary,
    _validate_database_metrics,
)


class ReleaseGateTests(unittest.TestCase):
    """Protect the comparison and conversion failure boundaries."""

    def test_differential_reports_every_source_target_and_relationship_delta(self) -> None:
        """Keep additions, omissions, partial overlap, and relationships visible."""
        shared = ("caller.py", (1, 0, 4))
        candidate = {
            shared: frozenset({("defs.py", (1, 0, 4)), ("defs.py", (2, 0, 4))}),
            ("candidate.py", (0, 0, 1)): frozenset({("defs.py", (3, 0, 1))}),
        }
        reference = {
            shared: frozenset({("defs.py", (1, 0, 4)), ("defs.py", (4, 0, 4))}),
            ("reference.py", (0, 0, 1)): frozenset({("defs.py", (5, 0, 1))}),
        }
        candidate_relationship = (
            ("child.py", (0, 0, 5)),
            ("base.py", (0, 0, 4)),
            ("is_implementation",),
        )
        reference_relationship = (
            ("child.py", (0, 0, 5)),
            ("protocol.py", (0, 0, 8)),
            ("is_implementation",),
        )

        report, missing = _differential(
            candidate,
            reference,
            frozenset({candidate_relationship}),
            frozenset({reference_relationship}),
        )

        self.assertEqual(missing, frozenset({shared}))
        self.assertEqual(len(report["candidate_only_sources"]), 1)
        self.assertEqual(len(report["reference_only_sources"]), 1)
        self.assertEqual(len(report["target_differences"]), 1)
        self.assertEqual(len(report["candidate_only_relationships"]), 1)
        self.assertEqual(len(report["reference_only_relationships"]), 1)

    def test_summary_and_database_metrics_reject_incomplete_results(self) -> None:
        """Require structured counters and nonempty converted query tables."""
        summary = _summary(
            "indexed 281 files: 22975 definitions, 53264 references; "
            "9751 unresolved, 39 ambiguous, 186 external, 3 skipped"
        )
        self.assertEqual(summary["ambiguous"], 39)
        good = {
            "documents": EXPECTED_DOCUMENTS,
            "chunks": 1,
            "mentions": 1,
            "defn_enclosing_ranges": 1,
            "global_symbols": 1,
        }
        _validate_database_metrics(good)
        with self.assertRaises(GateError):
            _validate_database_metrics({**good, "mentions": 0})


if __name__ == "__main__":
    unittest.main()
