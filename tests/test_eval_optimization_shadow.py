import importlib.util
import json
import os
import tempfile
import unittest


SCRIPT_PATH = os.path.join(
    os.path.dirname(__file__), "..", "script", "eval-optimization-shadow-evaluations.py"
)
SPEC = importlib.util.spec_from_file_location("eval_optimization_shadow", SCRIPT_PATH)
EVALUATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVALUATOR)


def row(evaluation_id=1, **overrides):
    result = {
        "id": evaluation_id,
        "simulationId": 11,
        "projectId": 7,
        "proposalId": 21,
        "sourceVersionId": 31,
        "candidateVersionId": 32,
        "outcome": "eligible_shadow_candidate",
        "findingCode": "HIGH_FAILURE_RATE",
        "sampleSource": "approved_business_tests",
        "testSetSize": 12,
        "baselineAccuracy": 92.0,
        "simulatedAccuracy": 94.0,
        "createdAt": "2026-11-14T00:00:00Z",
    }
    result.update(overrides)
    return result


class EvalOptimizationShadowTests(unittest.TestCase):
    def test_report_counts_gate_dimensions_and_accuracy_changes(self):
        records = [
            row(1, simulatedAccuracy=94.0),
            row(2, outcome="blocked_simulation", simulatedAccuracy=92.0),
            row(3, findingCode="HIGH_MISMATCH_RATE", sampleSource="successful_history_runs",
                simulatedAccuracy=90.0),
        ]
        report = EVALUATOR.build_report(records)
        self.assertEqual(report["evaluationCount"], 3)
        self.assertTrue(report["shadowQualificationIsNotProductionAcceptance"])
        self.assertEqual(report["distributions"]["outcome"]["eligible_shadow_candidate"], 2)
        self.assertEqual(report["distributions"]["sampleSource"]["successful_history_runs"], 1)
        self.assertEqual(report["simulationAccuracyChange"], {
            "count": 3,
            "accuracyChangePp": {
                "mean": 0.0, "improved": 1, "unchanged": 1,
                "regressed": 1, "denominator": 3,
            },
        })
        self.assertEqual(report["byOutcome"]["blocked_simulation"]["count"], 1)

    def test_validation_rejects_extra_sensitive_fields_and_invalid_values(self):
        invalid = [
            row(evidence={"input": "secret"}),
            row(id=True),
            row(outcome="accepted_in_production"),
            row(simulatedAccuracy=float("inf")),
            row(testSetSize=False),
        ]
        for record in invalid:
            with self.subTest(record=record), self.assertRaises(ValueError):
                EVALUATOR.validate_row(record, 1)

    def test_jsonl_loader_rejects_duplicate_ids_and_preserves_empty_report(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "input.jsonl")
            with open(path, "w", encoding="utf-8") as target:
                target.write(json.dumps(row(1)) + "\n")
                target.write(json.dumps(row(1)) + "\n")
            with self.assertRaisesRegex(ValueError, "duplicate evaluation ID"):
                EVALUATOR.load_records(path)
        empty = EVALUATOR.build_report([])
        self.assertEqual(empty["evaluationCount"], 0)
        self.assertIsNone(empty["simulationAccuracyChange"]["accuracyChangePp"]["mean"])

    def test_jsonl_loader_rejects_duplicate_object_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "input.jsonl")
            with open(path, "w", encoding="utf-8") as target:
                target.write('{"id":1,"id":2}\n')
            with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
                EVALUATOR.load_records(path)

    def test_report_file_is_private(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "report.json")
            EVALUATOR.write_json(path, EVALUATOR.build_report([row()]))
            self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)


if __name__ == "__main__":
    unittest.main()
