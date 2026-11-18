import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import urllib.parse


SPEC = importlib.util.spec_from_file_location(
    "export_deviation_inventory", Path(__file__).parents[1] / "script/export-deviation-inventory.py"
)
EXPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORTER)


def record(record_id=1, **values):
    return {"id": record_id, "projectId": 8, "decisionRunId": record_id + 100,
            "status": "candidate", "confidence": 0.75, "reviewLabel": "correct",
            "reviewedBy": "private-reviewer", "evidence": {"private": "business"},
            "attributedRule": "private-rule", **values}


class Opener:
    def __init__(self, pages):
        self.pages = iter(pages)
        self.requests = []

    def open(self, request, timeout):
        self.requests.append(request)
        page = next(self.pages)
        return io.BytesIO(page if isinstance(page, bytes) else json.dumps(page).encode())


class ExportDeviationInventoryTests(unittest.TestCase):
    def fetch(self, pages):
        opener = Opener(pages)
        with patch.object(EXPORTER.urllib.request, "build_opener", return_value=opener):
            return EXPORTER.fetch_inventory("http://localhost:3000", "test-token", 8), opener

    def test_exports_unreviewed_and_insufficient_cases_without_text_or_labels(self):
        page = {"throughId": 2, "records": [record(), record(2, status="insufficient_evidence")]}
        rows, opener = self.fetch([page, page])
        self.assertEqual(len(rows), 2)
        self.assertEqual(set(rows[0]), {"caseId", "decisionRunId", "status", "confidence"})
        self.assertNotIn("private", json.dumps(rows))
        self.assertEqual(rows[1]["status"], "insufficient_evidence")
        self.assertIn("throughId=2", opener.requests[1].full_url)
        self.assertEqual(opener.requests[0].get_header("Authorization"), "Bearer test-token")

    def test_inventory_feeds_accuracy_evaluator_without_excluding_unreviewed_cases(self):
        spec = importlib.util.spec_from_file_location(
            "deviation_inventory_evaluation_integration",
            Path(__file__).parents[1] / "script/eval-deviation-reviews.py",
        )
        evaluator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(evaluator)
        page = {"throughId": 2, "records": [record(), record(2)]}
        rows, _ = self.fetch([page, page])
        review = {**rows[0], "attributedRule": "private-rule", "label": "correct"}
        report = evaluator.evaluate([review], inventory=rows)
        self.assertEqual(report["samplingFrame"]["attributionRuns"], 2)
        self.assertEqual(report["samplingFrame"]["unreviewedRuns"], 1)
        self.assertEqual(report["samplingFrame"]["conclusiveCoverage"], 0.5)
        self.assertEqual(report["candidateAccuracyAssessment"]["status"], "insufficient_samples")
        self.assertFalse(report["m3Acceptance"]["passed"])
        self.assertNotIn("private-rule", json.dumps(report))

    def test_freezes_boundary_across_pages_and_rechecks_the_same_inventory(self):
        with patch.object(EXPORTER, "PAGE_SIZE", 1):
            pages = [{"throughId": 2, "records": [record()]},
                     {"throughId": 2, "records": [record(2)]},
                     {"throughId": 2, "records": []}] * 2
            rows, opener = self.fetch(pages)
        self.assertEqual(len(rows), 2)
        for request in opener.requests[1:]:
            self.assertIn("throughId=2", request.full_url)
        self.assertIn("afterId=1", opener.requests[1].full_url)

    def test_detects_changes_and_preserves_existing_output(self):
        before = {"throughId": 2, "records": [record(), record(2)]}
        after = {"throughId": 2, "records": [record()]}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "inventory.jsonl"
            output.write_text("old")
            with patch.object(EXPORTER.urllib.request, "build_opener", return_value=Opener([before, after])):
                with self.assertRaisesRegex(ValueError, "changed during export"):
                    EXPORTER.write_private_export(output, "http://localhost", "token", 8)
            self.assertEqual(output.read_text(), "old")
            self.assertEqual(len(list(Path(directory).iterdir())), 1)

    def test_validates_project_run_status_confidence_and_cursor(self):
        bad_records = [record(projectId=9), record(projectId=True), record(decisionRunId=True),
                       record(status=[]), record(confidence=True), record(confidence=2),
                       record(confidence=10**400),
                       record(0), record(3)]
        for bad in bad_records:
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                self.fetch([{"throughId": 2, "records": [bad]}])
        with self.assertRaisesRegex(ValueError, "unique"):
            self.fetch([{"throughId": 2, "records": [record(), record(2, decisionRunId=101)]}])

    def test_rejects_changed_boundary_and_duplicate_ids(self):
        with self.assertRaisesRegex(ValueError, "boundary"):
            self.fetch([{"throughId": 1, "records": [record()]}, {"throughId": 2, "records": []}])
        with self.assertRaisesRegex(ValueError, "advance"):
            self.fetch([{"throughId": 1, "records": [record(), record()]}])

    def test_rejects_invalid_json_and_oversized_responses(self):
        for body in [b'{"throughId":1,"throughId":2,"records":[]}',
                     b'{"throughId":1,"records":[],"x":NaN}', b'not-json', b'\xff']:
            with self.subTest(body=body), self.assertRaisesRegex(ValueError, "invalid JSON"):
                self.fetch([body])
        with patch.object(EXPORTER, "MAX_RESPONSE_BYTES", 2):
            with self.assertRaisesRegex(ValueError, "size limit"):
                self.fetch([b'1234'])

    def test_empty_project_and_record_limit(self):
        rows, _ = self.fetch([{"throughId": 0, "records": []}] * 2)
        self.assertEqual(rows, [])
        with patch.object(EXPORTER, "MAX_RECORDS", 1):
            with self.assertRaisesRegex(ValueError, "exceeds"):
                self.fetch([{"throughId": 2, "records": [record(), record(2)]}])

    def test_local_argument_validation_precedes_network(self):
        for url, token, project in [("http://user:password@host", "token", 8),
                                    ("http://host?key=secret", "token", 8),
                                    ("file:///tmp/input", "token", 8),
                                    ("http://host", "token\nsecret", 8),
                                    ("http://host", None, 8),
                                    ("http://host", "token", True)]:
            with patch.object(EXPORTER.urllib.request, "build_opener") as network:
                with self.assertRaises(ValueError):
                    EXPORTER.fetch_inventory(url, token, project)
                network.assert_not_called()

    def test_writes_private_file_atomically(self):
        page = {"throughId": 1, "records": [record()]}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "inventory.jsonl"
            with patch.object(EXPORTER.urllib.request, "build_opener", return_value=Opener([page, page])):
                self.assertEqual(EXPORTER.write_private_export(output, "http://localhost", "token", 8), 1)
            self.assertEqual(os.stat(output).st_mode & 0o777, 0o600)
            self.assertEqual(json.loads(output.read_text())["caseId"], "deviation-1")
            self.assertEqual(len(list(Path(directory).iterdir())), 1)

    def test_transport_errors_do_not_print_authenticated_url(self):
        opener = Opener([])
        with patch.object(opener, "open", side_effect=urllib.error.URLError("secret-token")):
            with patch.object(EXPORTER.urllib.request, "build_opener", return_value=opener):
                with self.assertRaisesRegex(ValueError, "request failed") as caught:
                    EXPORTER.fetch_inventory("http://localhost", "token", 8)
        self.assertNotIn("secret-token", str(caught.exception))
        self.assertIsNone(EXPORTER.NoRedirect().redirect_request(None, None, 302, "", {}, "https://other"))


if __name__ == "__main__":
    unittest.main()
