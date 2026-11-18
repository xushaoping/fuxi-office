import importlib.util
import json
import os
import tempfile
import unittest


SCRIPT_PATH = os.path.join(
    os.path.dirname(__file__), "..", "script", "compare-decision-kpi-baselines.py"
)
SPEC = importlib.util.spec_from_file_location("compare_decision_kpi_baselines", SCRIPT_PATH)
COMPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARE)


def snapshot(project_id=4, version_id=12, window_days=30, total_runs=20, failed_runs=2):
    succeeded_runs = total_runs - failed_runs
    outcomes = min(10, total_runs)
    conclusive = min(8, outcomes)
    mismatch = min(2, conclusive)
    summary = {
        "totalRuns": total_runs,
        "succeededRuns": succeeded_runs,
        "failedRuns": failed_runs,
        "outcomeReportedRuns": outcomes,
        "conclusiveOutcomeRuns": conclusive,
        "confirmedRuns": max(0, conclusive - mismatch),
        "mismatchRuns": mismatch,
        "inconclusiveRuns": outcomes - conclusive,
        "failureRate": failed_runs / total_runs if total_runs else 0,
        "feedbackCoverageRate": outcomes / succeeded_runs if succeeded_runs else 0,
        "mismatchRate": mismatch / conclusive if conclusive else 0,
        "averageDurationMs": 42.5,
        "p95DurationMs": 100,
    }
    return {
        "projectId": project_id,
        "versionId": version_id,
        "versionNumber": 3,
        "windowDays": window_days,
        "windowStart": "2026-10-16T00:00:00Z",
        "generatedAt": "2026-11-14T00:00:00Z",
        "collectedAt": "2026-11-14T00:01:00Z",
        "summary": summary,
        "sampleCounts": {
            "failureRate": total_runs,
            "feedbackCoverageRate": succeeded_runs,
            "mismatchRate": conclusive,
            "averageDurationMs": total_runs,
            "p95DurationMs": total_runs,
        },
        "privateEvidence": "should never appear in output",
    }


class CompareDecisionKpiBaselineTests(unittest.TestCase):
    def test_reports_counts_rate_denominators_and_latency_deltas(self):
        before = {(4, 12): COMPARE._validate_snapshot(snapshot())}
        after = {(4, 12): COMPARE._validate_snapshot(
            snapshot(total_runs=30, failed_runs=3)
        )}
        report = COMPARE.compare_snapshots(before, after)
        project = report["projects"][0]
        self.assertEqual(project["status"], "comparable")
        self.assertEqual(project["counts"]["totalRuns"]["delta"], 10)
        self.assertEqual(project["metrics"]["failureRate"]["beforeNumerator"], 2)
        self.assertEqual(project["metrics"]["failureRate"]["beforeDenominator"], 20)
        self.assertEqual(project["metrics"]["averageDurationMs"]["delta"], 0)
        self.assertEqual(project["metrics"]["averageDurationMs"]["beforeDenominator"], 20)

    def test_window_mismatch_suppresses_deltas_and_reports_status(self):
        before = {(4, 12): COMPARE._validate_snapshot(snapshot(window_days=7))}
        after = {(4, 12): COMPARE._validate_snapshot(snapshot(window_days=30))}
        project = COMPARE.compare_snapshots(before, after)["projects"][0]
        self.assertEqual(project["status"], "incomparable_window")
        self.assertIsNone(project["counts"]["totalRuns"]["delta"])
        self.assertIsNone(project["metrics"]["mismatchRate"]["delta"])
        self.assertFalse(project["metrics"]["mismatchRate"]["comparable"])

    def test_reports_unmatched_versions_and_stably_sorts_identities(self):
        before = {
            (9, 21): COMPARE._validate_snapshot(snapshot(9, 21)),
            (4, 12): COMPARE._validate_snapshot(snapshot()),
        }
        after = {(4, 13): COMPARE._validate_snapshot(snapshot(4, 13))}
        projects = COMPARE.compare_snapshots(before, after)["projects"]
        self.assertEqual(
            [(item["projectId"], item["versionId"], item["status"]) for item in projects],
            [(4, 12, "missing_after"), (4, 13, "missing_before"), (9, 21, "missing_after")],
        )

    def test_jsonl_rejects_duplicate_object_keys_and_duplicate_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "snapshot.jsonl")
            with open(path, "w", encoding="utf-8") as target:
                target.write('{"projectId":4,"projectId":5}\n')
            with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
                COMPARE._read_jsonl(path)
            with open(path, "w", encoding="utf-8") as target:
                line = json.dumps(snapshot())
                target.write(line + "\n" + line + "\n")
            with self.assertRaisesRegex(ValueError, "duplicate project/version"):
                COMPARE._read_jsonl(path)

    def test_validation_rejects_inconsistent_rates_and_denominators(self):
        invalid = snapshot()
        invalid["summary"]["failureRate"] = 0.5
        with self.assertRaisesRegex(ValueError, "does not match its count denominator"):
            COMPARE._validate_snapshot(invalid)
        invalid = snapshot()
        invalid["sampleCounts"]["p95DurationMs"] = 1
        with self.assertRaisesRegex(ValueError, "does not match its metric denominator"):
            COMPARE._validate_snapshot(invalid)

    def test_report_does_not_copy_unknown_sensitive_fields(self):
        before = {(4, 12): COMPARE._validate_snapshot(snapshot())}
        after = {(4, 12): COMPARE._validate_snapshot(snapshot())}
        self.assertNotIn(
            "privateEvidence",
            json.dumps(COMPARE.compare_snapshots(before, after)),
        )


if __name__ == "__main__":
    unittest.main()
