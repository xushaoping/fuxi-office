import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest


SCRIPT_PATH = os.path.join(os.path.dirname(__file__), "..", "script", "eval-drift.py")
SPEC = importlib.util.spec_from_file_location("eval_drift", SCRIPT_PATH)
EVAL_DRIFT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVAL_DRIFT)


def case(case_id, label, total_runs, conclusive, failure_rate, mismatch_rate, codes):
    return {
        "caseId": case_id,
        "label": label,
        "report": {
            "summary": {
                "totalRuns": total_runs,
                "conclusiveOutcomeRuns": conclusive,
                "failureRate": failure_rate,
                "mismatchRate": mismatch_rate,
            },
            "findings": [{"code": code} for code in codes],
        },
        "codes": set(codes),
    }


class DriftEvaluationTests(unittest.TestCase):
    def temporal_cases(self):
        items = [
            case("train-drift", "drift", 100, 50, 0.3, 0.0, []),
            case("train-normal", "normal", 100, 50, 0.15, 0.0, []),
            case("purged", "normal", 100, 50, 0.0, 0.0, []),
            case("test-drift", "drift", 100, 50, 0.15, 0.0, []),
            case("test-normal", "normal", 100, 50, 0.3, 0.0, []),
        ]
        for item, start, day in zip(items,
                                    ["2025-12-01", "2025-12-02", "2026-01-16", "2026-02-01", "2026-02-02"],
                                    ["2026-01-01", "2026-01-02", "2026-02-15", "2026-03-01", "2026-03-02"]):
            item["scanDay"] = day
            item["projectId"] = 7
            item["sourceVersionId"] = 12
            item["report"]["windowStart"] = start + "T00:00:00Z"
            item["report"]["generatedAt"] = day + "T12:00:00Z"
        return items

    def test_temporal_holdout_selects_only_on_training_and_purges_overlap(self):
        cases = self.temporal_cases()
        report = EVAL_DRIFT.evaluate_temporal_holdout(
            cases, "2026-03-01", [5], [0.2], [0.1, 0.2, 0.4], [0.25], [0.15]
        )
        self.assertEqual(report["trainingCases"], 2)
        self.assertEqual(report["holdoutCases"], 2)
        self.assertEqual(report["purgedCases"], 1)
        self.assertEqual(report["selectedThresholds"]["failureRateThreshold"], 0.2)
        self.assertEqual(report["trainingMetrics"]["f1"], 1.0)
        self.assertEqual(report["holdoutMetrics"]["f1"], 0.0)
        self.assertEqual(report["holdoutMetricsByProjectAndVersion"], [{
            "projectId": 7,
            "sourceVersionId": 12,
            "scope": "project and source version",
            **EVAL_DRIFT.score(
                [case for case in cases if case["scanDay"] >= "2026-03-01"],
                [False, True],
            ),
        }])
        cases[-1]["label"], cases[-2]["label"] = cases[-2]["label"], cases[-1]["label"]
        changed = EVAL_DRIFT.evaluate_temporal_holdout(
            cases, "2026-03-01", [5], [0.2], [0.1, 0.2, 0.4], [0.25], [0.15]
        )
        self.assertEqual(report["selectedThresholds"], changed["selectedThresholds"])

    def test_temporal_holdout_rejects_missing_dates_empty_training_and_one_class(self):
        for mutation, message in [
            (lambda cases: cases[0].pop("scanDay"), "requires scanDay"),
            (lambda cases: cases[0]["report"].update(generatedAt="2026-01-01T00:00:00"), "timezone-aware"),
            (lambda cases: cases[-1].update(label="drift"), "both drift and normal"),
            (lambda cases: cases[-1]["report"].update(windowStart="2025-01-01T00:00:00Z"), "no training cases"),
            (lambda cases: cases[-1].update(projectId=True), "projectId must be a positive integer"),
        ]:
            with self.subTest(message=message):
                cases = self.temporal_cases()
                mutation(cases)
                with self.assertRaisesRegex(ValueError, message):
                    EVAL_DRIFT.evaluate_temporal_holdout(
                        cases, "2026-03-01", [5], [0.2], [0.1], [0.25], [0.15]
                    )

    def test_temporal_holdout_cli_loads_export_format_and_rejects_event_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            dataset = os.path.join(directory, "windows.jsonl")
            output = os.path.join(directory, "evaluation.json")
            with open(dataset, "w", encoding="utf-8") as target:
                for item in self.temporal_cases():
                    item.pop("codes")
                    target.write(json.dumps(item) + "\n")
            command = [sys.executable, SCRIPT_PATH, dataset,
                       "--holdout-from", "2026-03-01", "--output", output]
            completed = subprocess.run(command, capture_output=True, text=True, timeout=10)
            self.assertEqual(completed.returncode, 0, completed.stderr)
            with open(output, encoding="utf-8") as source:
                report = json.load(source)
            self.assertEqual(report["purgedCases"], 1)
            self.assertEqual(report["holdoutMetrics"]["cases"], 2)
            rejected = subprocess.run(command + ["--reviewed-events"],
                                      capture_output=True, text=True, timeout=10)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn("requires full windows", rejected.stderr)

    def test_metrics_report_false_positives_and_false_negatives(self):
        cases = [
            case("a", "drift", 20, 10, 0.0, 0.4, ["HIGH_MISMATCH_RATE"]),
            case("b", "normal", 20, 10, 0.0, 0.1, ["LOW_FEEDBACK_COVERAGE"]),
            case("c", "drift", 20, 10, 0.0, 0.1, []),
        ]
        metrics = EVAL_DRIFT.score(cases, [bool(item["codes"]) for item in cases])
        self.assertEqual(metrics["truePositive"], 1)
        self.assertEqual(metrics["falsePositive"], 1)
        self.assertEqual(metrics["falseNegative"], 1)
        self.assertEqual(metrics["precision"], 0.5)
        self.assertEqual(metrics["recall"], 0.5)
        self.assertEqual(metrics["precisionDenominator"], 2)
        self.assertEqual(metrics["precisionWilson95"], [0.0945, 0.9055])
        self.assertEqual(metrics["recallWilson95"], [0.0945, 0.9055])

    def test_wilson_interval_handles_empty_and_perfect_small_samples(self):
        self.assertIsNone(EVAL_DRIFT.wilson_interval(0, 0))
        lower, upper = EVAL_DRIFT.wilson_interval(5, 5)
        self.assertLess(lower, 1.0)
        self.assertEqual(upper, 1.0)

    def test_threshold_sweep_applies_run_count_and_rate_thresholds(self):
        item = case("a", "drift", 20, 8, 0.15, 0.25, ["HIGH_FAILURE_RATE", "HIGH_MISMATCH_RATE"])
        self.assertTrue(EVAL_DRIFT.predict_threshold_sweep(item, 5, 0.2, 0.1))
        self.assertFalse(EVAL_DRIFT.predict_threshold_sweep(item, 10, 0.2, 0.2))
        self.assertFalse(EVAL_DRIFT.predict_threshold_sweep(item, 5, 0.3, 0.2))

    def test_distribution_threshold_sweep_replays_structured_signals_with_correction(self):
        drift_case = case("drift", "drift", 30, 25, 0.0, 0.0, ["INPUT_DISTRIBUTION_DRIFT"])
        normal_case = case("normal", "normal", 30, 25, 0.0, 0.0, ["INPUT_DISTRIBUTION_DRIFT"])
        for item, psi in ((drift_case, 0.4), (normal_case, 0.28)):
            item["hasDriftSignals"] = True
            item["driftSignals"] = [{
                "code": "INPUT_DISTRIBUTION_DRIFT",
                "element": "amount",
                "metricValue": psi,
                "pValue": 0.01,
                "baselineSampleCount": 40,
                "recentSampleCount": 30,
            }]

        self.assertTrue(EVAL_DRIFT.predict_threshold_sweep(
            drift_case, 5, 0.2, 0.1, psi_threshold=0.35
        ))
        self.assertFalse(EVAL_DRIFT.predict_threshold_sweep(
            normal_case, 5, 0.2, 0.1, psi_threshold=0.35
        ))

        drift_case["driftSignals"] *= 6
        self.assertFalse(EVAL_DRIFT.predict_threshold_sweep(
            drift_case, 5, 0.2, 0.1, psi_threshold=0.35
        ))

    def test_old_report_without_structured_signals_keeps_recorded_findings(self):
        item = case("legacy", "drift", 20, 8, 0.0, 0.0, ["INPUT_DISTRIBUTION_DRIFT"])
        self.assertTrue(EVAL_DRIFT.predict_threshold_sweep(item, 5, 0.2, 0.1, psi_threshold=10))

    def test_missing_rate_threshold_sweep_replays_missingness_gap(self):
        item = case("missing", "drift", 30, 25, 0.0, 0.0, ["INPUT_FIELD_MISSINGNESS_DRIFT"])
        item["hasDriftSignals"] = True
        item["driftSignals"] = [{
            "code": "INPUT_FIELD_MISSINGNESS_DRIFT",
            "element": "amount",
            "metricValue": 0.2,
            "pValue": 0.001,
            "baselineSampleCount": 50,
            "recentSampleCount": 30,
        }]
        self.assertTrue(EVAL_DRIFT.predict_threshold_sweep(
            item, 5, 0.2, 0.1, missing_rate_gap_threshold=0.15
        ))
        self.assertFalse(EVAL_DRIFT.predict_threshold_sweep(
            item, 5, 0.2, 0.1, missing_rate_gap_threshold=0.25
        ))

    def test_reviewed_event_report_only_exposes_confirmation_metrics(self):
        cases = [
            case("a", "drift", 20, 8, 0.0, 0.25, ["HIGH_MISMATCH_RATE"]),
            case("b", "normal", 20, 8, 0.0, 0.25, ["HIGH_MISMATCH_RATE"]),
        ]
        report = EVAL_DRIFT.evaluate_reviewed_events(cases)
        self.assertEqual(report["overall"]["confirmationRate"], 0.5)
        self.assertEqual(report["perSignal"]["HIGH_MISMATCH_RATE"]["falsePositive"], 1)
        self.assertNotIn("recall", report["overall"])
        self.assertIn("confirmationRateWilson95", report["overall"])

    def test_full_window_evaluation_returns_metrics_and_threshold_sweep(self):
        cases = [
            case("a", "drift", 20, 8, 0.0, 0.25, ["HIGH_MISMATCH_RATE"]),
            case("b", "normal", 20, 8, 0.0, 0.0, []),
        ]
        report = EVAL_DRIFT.evaluate(cases, [5], [0.2], [0.1])
        self.assertEqual(report["caseCount"], 2)
        self.assertEqual(report["currentFindings"]["truePositive"], 1)
        self.assertEqual(len(report["thresholdSweep"]), 1)
        self.assertIn("psiThreshold", report["thresholdSweep"][0])

    def test_loader_rejects_invalid_drift_signal_probabilities(self):
        row = case("a", "drift", 20, 10, 0.0, 0.0, [])
        row.pop("codes")
        row["report"]["driftSignals"] = [{
            "code": "INPUT_DISTRIBUTION_DRIFT",
            "element": "amount",
            "metricValue": 0.3,
            "pValue": 1.1,
            "baselineSampleCount": 20,
            "recentSampleCount": 20,
        }]
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write(json.dumps(row) + "\n")
            fixture.flush()
            with self.assertRaisesRegex(ValueError, "pValue"):
                EVAL_DRIFT.load_cases(fixture.name)

    def test_loader_rejects_duplicate_ids_and_invalid_rates(self):
        row = case("a", "normal", 10, 5, 0.0, 0.0, [])
        row.pop("codes")
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write(json.dumps(row) + "\n")
            fixture.write(json.dumps(row) + "\n")
            fixture.flush()
            with self.assertRaisesRegex(ValueError, "duplicate caseId"):
                EVAL_DRIFT.load_cases(fixture.name)

        row["report"]["summary"]["failureRate"] = 1.1
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write(json.dumps(row) + "\n")
            fixture.flush()
            with self.assertRaisesRegex(ValueError, "failureRate"):
                EVAL_DRIFT.load_cases(fixture.name)

    def test_loader_rejects_non_object_rows_and_boolean_counts(self):
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write("[]\n")
            fixture.flush()
            with self.assertRaisesRegex(ValueError, "JSON object"):
                EVAL_DRIFT.load_cases(fixture.name)

        row = case("a", "normal", 10, 5, 0.0, 0.0, [])
        row.pop("codes")
        row["report"]["summary"]["totalRuns"] = True
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write(json.dumps(row) + "\n")
            fixture.flush()
            with self.assertRaisesRegex(ValueError, "totalRuns"):
                EVAL_DRIFT.load_cases(fixture.name)

    def test_score_rejects_prediction_count_mismatch(self):
        with self.assertRaisesRegex(ValueError, "prediction count"):
            EVAL_DRIFT.score([case("a", "drift", 1, 1, 0.0, 1.0, [])], [])


if __name__ == "__main__":
    unittest.main()
