import importlib.util
import io
import json
import os
import unittest
from unittest.mock import patch


SCRIPT_PATH = os.path.join(os.path.dirname(__file__), "..", "script", "export-drift-reviews.py")
SPEC = importlib.util.spec_from_file_location("export_drift_reviews", SCRIPT_PATH)
EXPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORTER)


class ExportDriftReviewsTests(unittest.TestCase):
    def test_export_skips_inconclusive_and_maps_review_labels(self):
        events = [
            {
                "id": 3,
                "reviewLabel": "confirmed_drift",
                "evidence": {
                    "summary": {"totalRuns": 30, "conclusiveOutcomeRuns": 8, "failureRate": 0.0, "mismatchRate": 0.25},
                    "findings": [{"code": "HIGH_MISMATCH_RATE"}],
                },
            },
            {"id": 4, "reviewLabel": "inconclusive", "evidence": {}},
            {
                "id": 5,
                "reviewLabel": "false_positive",
                "evidence": {
                    "summary": {"totalRuns": 30, "conclusiveOutcomeRuns": 8, "failureRate": 0.0, "mismatchRate": 0.25},
                    "findings": [{"code": "HIGH_MISMATCH_RATE"}],
                },
            },
        ]
        response = io.BytesIO(json.dumps(events).encode("utf-8"))
        response.__enter__ = lambda: response
        response.__exit__ = lambda *_: None
        output = io.StringIO()
        with patch.object(EXPORTER.urllib.request, "urlopen", return_value=response):
            count = EXPORTER.export_reviews("http://localhost:3000", "token", 7, output)
        rows = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(count, 2)
        self.assertEqual([row["label"] for row in rows], ["drift", "normal"])
        self.assertEqual(rows[1]["caseId"], "event-5")

    def test_export_windows_includes_untriggered_normal_snapshots(self):
        windows = [
            {
                "id": 2,
                "projectId": 7,
                "sourceVersionId": 19,
                "scanDay": "2026-11-12",
                "reviewLabel": "normal",
                "evidence": {"summary": {"totalRuns": 30, "conclusiveOutcomeRuns": 8,
                                           "failureRate": 0.0, "mismatchRate": 0.0}, "findings": []},
            },
            {
                "id": 3,
                "projectId": 7,
                "sourceVersionId": 19,
                "scanDay": "2026-11-13",
                "reviewLabel": "inconclusive",
                "evidence": {"summary": {}},
            },
        ]
        response = io.BytesIO(json.dumps(windows).encode("utf-8"))
        response.__enter__ = lambda: response
        response.__exit__ = lambda *_: None
        output = io.StringIO()
        with patch.object(EXPORTER.urllib.request, "urlopen", return_value=response):
            count = EXPORTER.export_windows("http://localhost:3000", "token", 7, output)
        rows = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertEqual(count, 1)
        self.assertEqual(rows[0]["label"], "normal")
        self.assertEqual(rows[0]["report"]["findings"], [])
        self.assertIn("2026-11-12", rows[0]["caseId"])
        self.assertEqual(rows[0]["scanDay"], "2026-11-12")
        self.assertEqual(rows[0]["projectId"], 7)
        self.assertEqual(rows[0]["sourceVersionId"], 19)


if __name__ == "__main__":
    unittest.main()
