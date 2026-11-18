import importlib.util
import io
import json
import os
import tempfile
import unittest
from unittest.mock import patch


SCRIPT_PATH = os.path.join(
    os.path.dirname(__file__), "..", "script", "export-optimization-shadow-evaluations.py"
)
SPEC = importlib.util.spec_from_file_location("export_optimization_shadow", SCRIPT_PATH)
EXPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORTER)


def evaluation(evaluation_id=1, **overrides):
    record = {
        "id": evaluation_id,
        "simulationId": 10 + evaluation_id,
        "projectId": 7,
        "proposalId": 20 + evaluation_id,
        "sourceVersionId": 30,
        "candidateVersionId": 31,
        "outcome": "eligible_shadow_candidate",
        "findingCode": "HIGH_FAILURE_RATE",
        "sampleSource": "approved_business_tests",
        "testSetSize": 12,
        "baselineAccuracy": 92.5,
        "simulatedAccuracy": 93.0,
        "createdAt": "2026-11-14T00:00:00Z",
        "evidence": {"privateInput": "should-not-export"},
    }
    record.update(overrides)
    return record


class Response:
    def __init__(self, value):
        self.body = json.dumps(value).encode()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return None

    def read(self, limit):
        return self.body[:limit]


class ExportOptimizationShadowTests(unittest.TestCase):
    def test_export_keeps_summary_allowlist_and_paginates(self):
        requests = []
        pages = [[evaluation(index) for index in range(1, EXPORTER.PAGE_SIZE + 1)], []]

        def fake_open(request):
            requests.append(request)
            return Response(pages.pop(0))

        output = io.StringIO()
        with patch.object(EXPORTER, "open_url", side_effect=fake_open):
            count = EXPORTER.export_evaluations("https://api.example", "token", 7, output)
        rows = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(count, EXPORTER.PAGE_SIZE)
        self.assertEqual(len(requests), 2)
        self.assertIn("afterId=0", requests[0].full_url)
        self.assertIn(f"afterId={EXPORTER.PAGE_SIZE}", requests[1].full_url)
        self.assertNotIn("evidence", rows[0])
        self.assertNotIn("privateInput", output.getvalue())
        self.assertEqual(rows[0]["outcome"], "eligible_shadow_candidate")

    def test_validation_rejects_wrong_project_bad_enums_and_non_finite_metrics(self):
        for record in [
            evaluation(projectId=8),
            evaluation(outcome=[]),
            evaluation(outcome={"unexpected": "value"}),
            evaluation(findingCode="UNKNOWN"),
            evaluation(sampleSource="raw_run_inputs"),
            evaluation(testSetSize=True),
            evaluation(simulationId=EXPORTER.MAX_I64 + 1),
            evaluation(candidateVersionId=EXPORTER.MAX_I64 + 1),
            evaluation(baselineAccuracy=float("nan")),
            evaluation(simulatedAccuracy=101),
        ]:
            with self.subTest(record=record):
                with self.assertRaises(ValueError):
                    EXPORTER.validate_evaluation(record, 7, 0)

    def test_invalid_or_unordered_ids_and_empty_token_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "unordered ID"):
            EXPORTER.validate_evaluation(evaluation(1), 7, 1)
        with self.assertRaisesRegex(ValueError, "invalid ID"):
            EXPORTER.validate_evaluation(evaluation(id=True), 7, 0)
        with self.assertRaisesRegex(ValueError, "positive integer"):
            EXPORTER.export_evaluations(
                "http://localhost", "token", EXPORTER.MAX_I64 + 1, io.StringIO()
            )
        with self.assertRaisesRegex(ValueError, "API_TOKEN"):
            EXPORTER.export_evaluations("http://localhost", " ", 7, io.StringIO())

    def test_output_is_private_and_failed_export_preserves_existing_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "shadow.jsonl")
            with patch.object(EXPORTER, "fetch_page", return_value=[evaluation()]):
                self.assertEqual(EXPORTER.write_export("http://localhost", "token", 7, path), 1)
            self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)
            with open(path, encoding="utf-8") as source:
                self.assertNotIn("privateInput", source.read())
            with open(path, "w", encoding="utf-8") as target:
                target.write("previous\n")
            with patch.object(EXPORTER, "fetch_page", side_effect=RuntimeError("API failed")):
                with self.assertRaisesRegex(RuntimeError, "API failed"):
                    EXPORTER.write_export("http://localhost", "token", 7, path)
            with open(path, encoding="utf-8") as source:
                self.assertEqual(source.read(), "previous\n")


if __name__ == "__main__":
    unittest.main()
