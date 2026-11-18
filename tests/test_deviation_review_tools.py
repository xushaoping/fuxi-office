import importlib.util
import io
import json
import os
import tempfile
import unittest
from unittest.mock import patch


ROOT = os.path.dirname(__file__)


def load_script(name, module_name):
    path = os.path.join(ROOT, "..", "script", name)
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


EXPORTER = load_script("export-deviation-reviews.py", "export_deviation_reviews")
EVALUATOR = load_script("eval-deviation-reviews.py", "eval_deviation_reviews")


def reviewed_case(run_id=1, label="correct", status="candidate", **overrides):
    record = {
        "caseId": f"deviation-{run_id}", "decisionRunId": run_id,
        "status": status, "attributedRule": "rule-a" if status == "candidate" else None,
        "label": label, "confidence": 0.75,
    }
    record.update(overrides)
    return record


def inventory_case(run_id=1, **overrides):
    record = reviewed_case(run_id, **overrides)
    return {key: record[key] for key in EVALUATOR.INVENTORY_FIELDS}


class DeviationReviewToolTests(unittest.TestCase):
    def test_export_keeps_only_review_metadata_and_advances_cursor(self):
        records = [
            {
                "id": 4,
                "decisionRunId": 104,
                "status": "candidate",
                "attributedRule": "rule-a",
                "confidence": 0.35,
                "reviewLabel": "correct",
                "reviewedBy": "reviewer-secret",
                "evidence": {"actualOutcome": {"secret": "value"}},
            },
            {
                "id": 9,
                "decisionRunId": 109,
                "status": "insufficient_evidence",
                "attributedRule": None,
                "confidence": 0.0,
                "reviewLabel": "inconclusive",
            },
        ]
        response = io.BytesIO(json.dumps(records).encode("utf-8"))
        response.__enter__ = lambda: response
        response.__exit__ = lambda *_: None
        output = io.StringIO()
        with patch.object(EXPORTER.urllib.request, "urlopen", return_value=response):
            count = EXPORTER.export_reviews("http://localhost:3000", "token", 8, output)

        rows = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(count, 2)
        self.assertEqual([row["caseId"] for row in rows], ["deviation-4", "deviation-9"])
        self.assertNotIn("reviewedBy", rows[0])
        self.assertNotIn("evidence", rows[0])

    def test_evaluation_excludes_inconclusive_cases_from_top1_accuracy(self):
        cases = [
            {"caseId": "a", "decisionRunId": 1, "attributedRule": "rule-a", "status": "candidate", "label": "correct", "confidence": 0.9},
            {"caseId": "b", "decisionRunId": 2, "attributedRule": "rule-b", "status": "candidate", "label": "incorrect", "confidence": 0.4},
            {"caseId": "c", "decisionRunId": 3, "attributedRule": "rule-c", "status": "candidate", "label": "inconclusive", "confidence": 0.5},
            {"caseId": "d", "decisionRunId": 4, "attributedRule": None, "status": "insufficient_evidence", "label": "inconclusive", "confidence": 0.0},
        ]
        report = EVALUATOR.evaluate(cases)
        self.assertEqual(report["top1Accuracy"], 0.5)
        self.assertEqual(report["conclusiveCases"], 2)
        self.assertEqual(report["inconclusive"], 2)
        self.assertEqual(report["conclusiveCoverage"], 0.5)
        self.assertNotIn("recall", report)
        calibration = report["confidenceCalibration"]
        self.assertEqual(calibration["calibrationCases"], 2)
        self.assertEqual(calibration["brierScore"], 0.085)
        self.assertEqual(calibration["expectedCalibrationError"], 0.25)
        self.assertEqual(calibration["bins"][2]["samples"], 1)
        self.assertTrue(calibration["bins"][2]["smallSample"])
        self.assertEqual(calibration["bins"][4]["samples"], 1)

    def test_calibration_uses_conclusive_candidates_and_includes_confidence_one(self):
        cases = [
            {"caseId": "a", "decisionRunId": 1, "attributedRule": "rule-a", "status": "candidate", "label": "correct", "confidence": 1.0},
            {"caseId": "b", "decisionRunId": 2, "attributedRule": "rule-b", "status": "candidate", "label": "incorrect", "confidence": 0.8},
            {"caseId": "c", "decisionRunId": 3, "attributedRule": "rule-c", "status": "candidate", "label": "inconclusive", "confidence": 0.99},
            {"caseId": "d", "decisionRunId": 4, "attributedRule": None, "status": "insufficient_evidence", "label": "inconclusive", "confidence": 0.0},
        ]
        calibration = EVALUATOR.evaluate(cases)["confidenceCalibration"]
        self.assertEqual(calibration["calibrationCases"], 2)
        self.assertEqual(calibration["bins"][4]["samples"], 2)
        self.assertEqual(calibration["bins"][4]["observedAccuracy"], 0.5)

    def test_calibration_is_undefined_without_conclusive_candidates(self):
        cases = [
            {"caseId": "a", "decisionRunId": 1, "attributedRule": "rule-a", "status": "candidate", "label": "inconclusive", "confidence": 0.9},
            {"caseId": "b", "decisionRunId": 2, "attributedRule": None, "status": "insufficient_evidence", "label": "inconclusive", "confidence": 0.0},
        ]
        calibration = EVALUATOR.evaluate(cases)["confidenceCalibration"]
        self.assertIsNone(calibration["brierScore"])
        self.assertIsNone(calibration["expectedCalibrationError"])
        self.assertTrue(all(item["smallSample"] for item in calibration["bins"]))

    def test_numerical_gate_requires_one_hundred_conclusive_runs_and_exact_target(self):
        insufficient = EVALUATOR.evaluate([reviewed_case(index + 1) for index in range(99)])
        self.assertEqual(insufficient["candidateAccuracyAssessment"]["status"], "insufficient_samples")
        cases = [reviewed_case(index + 1, "correct" if index < 75 else "incorrect") for index in range(100)]
        report = EVALUATOR.evaluate(cases)
        self.assertEqual(report["candidateAccuracyAssessment"]["status"], "target_met")
        self.assertEqual(report["top1Accuracy"], 0.75)
        self.assertEqual(report["uniqueDecisionRunCount"], 100)
        self.assertEqual(report["candidateAccuracyAssessment"]["minimumConclusiveRuns"], 100)
        self.assertEqual(report["candidateAccuracyAssessment"]["targetAccuracy"], 0.75)
        self.assertEqual(report["m3Acceptance"]["status"], "not_established")
        self.assertFalse(report["m3Acceptance"]["passed"])
        self.assertFalse(report["m3Acceptance"]["independentBlindReviewEvidence"])
        self.assertLess(report["top1AccuracyWilson95"]["lower"], 0.75)
        self.assertGreater(report["top1AccuracyWilson95"]["upper"], 0.75)
        cases[74]["label"] = "incorrect"
        self.assertEqual(EVALUATOR.evaluate(cases)["candidateAccuracyAssessment"]["status"], "target_not_met")

    def test_inconclusive_or_repeated_run_cannot_inflate_independent_sample_count(self):
        cases = [reviewed_case(index + 1, label="inconclusive") for index in range(100)]
        report = EVALUATOR.evaluate(cases)
        self.assertEqual(report["candidateAccuracyAssessment"]["status"], "insufficient_samples")
        self.assertIsNone(report["top1AccuracyWilson95"])
        self.assertIsNone(report["top1Accuracy"])
        with self.assertRaisesRegex(ValueError, "duplicate decisionRunId"):
            EVALUATOR.evaluate([reviewed_case(1), reviewed_case(1, caseId="another-case")])
        with self.assertRaisesRegex(ValueError, "decisionRunId"):
            EVALUATOR.evaluate([{key: value for key, value in reviewed_case().items() if key != "decisionRunId"}])
        empty = EVALUATOR.evaluate([])
        self.assertEqual(empty["candidateAccuracyAssessment"]["status"], "insufficient_samples")
        self.assertIsNone(empty["top1AccuracyWilson95"])

    def test_inventory_reports_unreviewed_and_overall_conclusive_coverage(self):
        inventory = [inventory_case(index + 1) for index in range(4)]
        cases = [reviewed_case(1), reviewed_case(2, label="inconclusive")]
        report = EVALUATOR.evaluate(cases, inventory=inventory)
        self.assertEqual(report["conclusiveCoverage"], 0.5)
        self.assertEqual(report["samplingFrame"], {
            "inventoryProvided": True, "scope": "supplied_attribution_inventory",
            "attributionRuns": 4, "reviewedRuns": 2, "unreviewedRuns": 2,
            "reviewCoverage": 0.5, "conclusiveCoverage": 0.25,
            "unreviewedCountKnown": True, "completenessVerified": False,
        })
        without_inventory = EVALUATOR.evaluate(cases)["samplingFrame"]
        self.assertFalse(without_inventory["inventoryProvided"])
        self.assertIsNone(without_inventory["attributionRuns"])
        self.assertIsNone(without_inventory["unreviewedRuns"])
        self.assertIsNone(without_inventory["conclusiveCoverage"])
        self.assertFalse(without_inventory["unreviewedCountKnown"])
        self.assertIsNone(EVALUATOR.evaluate([], inventory=[])["samplingFrame"]["reviewCoverage"])

    def test_inventory_requires_snapshot_identity_and_unique_runs(self):
        case = reviewed_case()
        for override in ({"caseId": "unknown"}, {"decisionRunId": 2},
                         {"confidence": 0.5}, {"status": "insufficient_evidence"}):
            with self.subTest(override=override), self.assertRaises(ValueError):
                EVALUATOR.evaluate([case], inventory=[inventory_case(**override)])
        with self.assertRaisesRegex(ValueError, "absent"):
            EVALUATOR.evaluate([case], inventory=[])
        with self.assertRaisesRegex(ValueError, "duplicate decisionRunId"):
            EVALUATOR.evaluate([], inventory=[inventory_case(), inventory_case(caseId="other")])
        with self.assertRaisesRegex(ValueError, "fields must be exactly"):
            EVALUATOR.evaluate([], inventory=[dict(inventory_case(), label="correct")])

    def test_report_omits_ids_rules_and_business_payloads(self):
        case = reviewed_case(12345, caseId="private-case", attributedRule="private-rule")
        rendered = json.dumps(EVALUATOR.evaluate([case], inventory=[{key: case[key] for key in EVALUATOR.INVENTORY_FIELDS}]))
        self.assertNotIn("private-case", rendered)
        self.assertNotIn("private-rule", rendered)
        self.assertNotIn("12345", rendered)
        with self.assertRaises(ValueError):
            EVALUATOR.evaluate([dict(case, businessPayload="secret")])

    def test_evaluator_validates_programmatic_values_and_gate_parameters(self):
        for invalid in (reviewed_case(decisionRunId=True), reviewed_case(decisionRunId=EVALUATOR.MAX_I64 + 1),
                        reviewed_case(confidence=float("nan")), reviewed_case(confidence=True),
                        reviewed_case(confidence=10**1000), reviewed_case(label=[])):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                EVALUATOR.evaluate([invalid])
        for minimum in (0, -1, True, 1.5):
            with self.subTest(minimum=minimum), self.assertRaises(ValueError):
                EVALUATOR.evaluate([], minimum_runs=minimum)
        for target in (-1, 2, True, float("inf"), float("nan"), 10**1000):
            with self.subTest(target=target), self.assertRaises(ValueError):
                EVALUATOR.evaluate([], target_accuracy=target)

    def test_jsonl_rejects_duplicate_keys_and_bounded_input(self):
        invalid = [
            ('{"caseId":"a","caseId":"b"}', "duplicate JSON key"),
            ("x" * (EVALUATOR.MAX_LINE_BYTES + 1), "line exceeds"),
        ]
        for raw, expected in invalid:
            with self.subTest(expected=expected), tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
                fixture.write(raw + "\n")
                fixture.flush()
                with self.assertRaisesRegex(ValueError, expected):
                    EVALUATOR.load_cases(fixture.name)
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write(json.dumps(reviewed_case(1)) + "\n" + json.dumps(reviewed_case(2)) + "\n")
            fixture.flush()
            with patch.object(EVALUATOR, "MAX_RECORDS", 1), self.assertRaisesRegex(ValueError, "exceeds 1 records"):
                EVALUATOR.load_cases(fixture.name)

    def test_cli_inventory_and_explicit_diagnostic_thresholds(self):
        with tempfile.TemporaryDirectory() as directory:
            reviewed_path = os.path.join(directory, "reviewed.jsonl")
            inventory_path = os.path.join(directory, "inventory.jsonl")
            output_path = os.path.join(directory, "report.json")
            with open(reviewed_path, "w", encoding="utf-8") as target:
                target.write(json.dumps(reviewed_case()) + "\n")
            with open(inventory_path, "w", encoding="utf-8") as target:
                target.write(json.dumps(inventory_case()) + "\n")
            EVALUATOR.main([reviewed_path, "--inventory", inventory_path, "--minimum-runs", "1", "--target-accuracy", "1", "--output", output_path])
            with open(output_path, encoding="utf-8") as source:
                report = json.load(source)
            self.assertEqual(report["candidateAccuracyAssessment"]["status"], "target_met")
            self.assertFalse(report["m3Acceptance"]["passed"])
            self.assertEqual(report["m3Acceptance"]["requiredIndependentConclusiveRuns"], 100)
            self.assertTrue(report["samplingFrame"]["inventoryProvided"])

    def test_loader_rejects_invalid_candidate_and_inconsistent_inconclusive_label(self):
        invalid_rows = [
            {"caseId": "a", "decisionRunId": 1, "status": "candidate", "label": "correct", "confidence": 0.5},
            {"caseId": "b", "decisionRunId": 2, "status": "insufficient_evidence", "label": "incorrect",
             "confidence": 0.0, "attributedRule": None},
        ]
        for row, expected in zip(invalid_rows, ("attributedRule", "must be inconclusive")):
            with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
                fixture.write(json.dumps(row) + "\n")
                fixture.flush()
                with self.assertRaisesRegex(ValueError, expected):
                    EVALUATOR.load_cases(fixture.name)


if __name__ == "__main__":
    unittest.main()
