import importlib.util
import json
import os
import tempfile
import unittest
from unittest.mock import patch


SCRIPT_PATH = os.path.join(os.path.dirname(__file__), "..", "script", "export-decision-kpi-baseline.py")
SPEC = importlib.util.spec_from_file_location("export_decision_kpi_baseline", SCRIPT_PATH)
EXPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORTER)


def report(project_id=4):
    return {
        "projectId": project_id,
        "versionId": 12,
        "versionNumber": 3,
        "windowDays": 30,
        "windowStart": "2026-10-16T00:00:00Z",
        "generatedAt": "2026-11-14T00:00:00Z",
        "summary": {
            "totalRuns": 20,
            "succeededRuns": 18,
            "failedRuns": 2,
            "outcomeReportedRuns": 10,
            "conclusiveOutcomeRuns": 8,
            "confirmedRuns": 6,
            "mismatchRuns": 2,
            "inconclusiveRuns": 2,
            "failureRate": 0.1,
            "feedbackCoverageRate": 10 / 18,
            "mismatchRate": 0.25,
            "averageDurationMs": 42.5,
            "p95DurationMs": 100,
        },
        "findings": [{"privateEvidence": "not exported"}],
        "dailyTrends": [{"output": "not exported"}],
    }


class DecisionKpiBaselineTests(unittest.TestCase):
    def test_normalization_is_project_scoped_and_allowlists_summary_fields(self):
        snapshot = EXPORTER.normalize_report(report(), 4, "fixed-time")
        self.assertEqual(snapshot["collectedAt"], "fixed-time")
        self.assertEqual(snapshot["summary"]["totalRuns"], 20)
        self.assertEqual(
            snapshot["sampleCounts"],
            {
                "failureRate": 20,
                "feedbackCoverageRate": 18,
                "mismatchRate": 8,
                "averageDurationMs": 20,
                "p95DurationMs": 20,
            },
        )
        self.assertNotIn("findings", snapshot)
        self.assertNotIn("dailyTrends", snapshot)
        self.assertNotIn("privateEvidence", json.dumps(snapshot))

    def test_normalization_rejects_project_mismatch_bad_denominators_and_boolean_rates(self):
        with self.assertRaisesRegex(ValueError, "does not match"):
            EXPORTER.normalize_report(report(project_id=5), 4)
        invalid_count = report()
        invalid_count["summary"]["outcomeReportedRuns"] = 21
        with self.assertRaisesRegex(ValueError, "exceeds totalRuns"):
            EXPORTER.normalize_report(invalid_count, 4)
        invalid_rate = report()
        invalid_rate["summary"]["failureRate"] = True
        with self.assertRaisesRegex(ValueError, "finite rate"):
            EXPORTER.normalize_report(invalid_rate, 4)
        inconsistent_rate = report()
        inconsistent_rate["summary"]["mismatchRate"] = 0.2
        with self.assertRaisesRegex(ValueError, "does not match its count denominator"):
            EXPORTER.normalize_report(inconsistent_rate, 4)

    def test_project_ids_reject_duplicates_and_non_positive_values(self):
        self.assertEqual(EXPORTER.parse_project_ids("4, 9"), [4, 9])
        for value in ("4,4", "0", "-2", "abc", "9223372036854775808"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                EXPORTER.parse_project_ids(value)

    def test_export_writes_one_private_jsonl_snapshot_per_project(self):
        with tempfile.TemporaryDirectory() as directory:
            output = os.path.join(directory, "baseline.jsonl")
            with patch.object(EXPORTER, "fetch_report", side_effect=[report(4), report(9)]) as fetch:
                EXPORTER.export_baseline("http://localhost", "secret", [4, 9], output)
            with open(output, encoding="utf-8") as source:
                rows = [json.loads(line) for line in source]
            with open(output, encoding="utf-8") as source:
                contents = source.read()
            self.assertEqual([row["projectId"] for row in rows], [4, 9])
            self.assertEqual(fetch.call_count, 2)
            self.assertEqual(os.stat(output).st_mode & 0o777, 0o600)
            self.assertNotIn("secret", contents)

    def test_failed_project_does_not_replace_existing_snapshot(self):
        with tempfile.TemporaryDirectory() as directory:
            output = os.path.join(directory, "baseline.jsonl")
            with open(output, "w", encoding="utf-8") as target:
                target.write("previous snapshot\n")
            with patch.object(EXPORTER, "fetch_report", return_value=report(project_id=99)):
                with self.assertRaisesRegex(RuntimeError, "does not match"):
                    EXPORTER.export_baseline("http://localhost", "secret", [4], output)
            with open(output, encoding="utf-8") as source:
                self.assertEqual(source.read(), "previous snapshot\n")
            self.assertEqual(os.listdir(directory), ["baseline.jsonl"])


if __name__ == "__main__":
    unittest.main()
