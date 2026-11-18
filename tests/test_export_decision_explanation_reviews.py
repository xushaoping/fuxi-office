import importlib.util
import io
import json
import os
import tempfile
import unittest
from unittest.mock import patch


SCRIPT_PATH = os.path.join(
    os.path.dirname(__file__), "..", "script", "export-decision-explanation-reviews.py"
)
SPEC = importlib.util.spec_from_file_location("export_explanation_reviews", SCRIPT_PATH)
EXPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORTER)


def api_row(record_id=1, explanation_id=4, reviewer="reviewer-a", label="readable", revision=1):
    return {
        "id": record_id,
        "explanationId": explanation_id,
        "explanationRevision": revision,
        "reviewerUsername": reviewer,
        "reviewLabel": label,
    }


class ExportDecisionExplanationReviewsTests(unittest.TestCase):
    def test_export_uses_cursor_and_writes_only_evaluation_fields(self):
        first_page = [api_row(index + 1, index + 100) for index in range(EXPORTER.PAGE_SIZE)]
        with patch.object(EXPORTER.urllib.request, "urlopen", side_effect=[
            io.BytesIO(json.dumps(first_page).encode()),
            io.BytesIO(json.dumps([api_row(501, 900)]).encode()),
        ]) as urlopen:
            output = io.StringIO()
            count = EXPORTER.export_reviews("http://example.test", "token", 9, output)

        self.assertEqual(count, 501)
        self.assertIn("afterId=0", urlopen.call_args_list[0].args[0].full_url)
        self.assertIn("afterId=500", urlopen.call_args_list[1].args[0].full_url)
        first_export = json.loads(output.getvalue().splitlines()[0])
        self.assertEqual(first_export, {
            "explanationId": 100, "explanationRevision": 1, "reviewer": "reviewer-a", "label": "readable",
        })
        self.assertNotIn("createdAt", output.getvalue())

    def test_export_rejects_unordered_rows_and_bad_labels(self):
        cases = (
            [api_row(2), api_row(1, explanation_id=5)],
            [api_row(label="not-a-label")],
            [api_row(), api_row(2)],
        )
        for records in cases:
            with self.subTest(records=records), patch.object(
                EXPORTER.urllib.request, "urlopen",
                return_value=io.BytesIO(json.dumps(records).encode()),
            ), self.assertRaises(RuntimeError):
                EXPORTER.export_reviews("http://example.test", "token", 9, io.StringIO())

    def test_export_keeps_revision_identity_and_rejects_unknown_revisions(self):
        records = [api_row(1, revision=1), api_row(2, revision=2)]
        with patch.object(EXPORTER.urllib.request, "urlopen", return_value=io.BytesIO(json.dumps(records).encode())):
            output = io.StringIO()
            self.assertEqual(EXPORTER.export_reviews("http://example.test", "token", 9, output), 2)
            self.assertEqual([json.loads(line)["explanationRevision"] for line in output.getvalue().splitlines()], [1, 2])
        for revision in (None, True, 0, -1, "1", 9_223_372_036_854_775_808):
            with self.subTest(revision=revision), patch.object(
                EXPORTER.urllib.request, "urlopen", return_value=io.BytesIO(json.dumps([api_row(revision=revision)]).encode()),
            ), self.assertRaisesRegex(RuntimeError, "invalid explanation revision"):
                EXPORTER.export_reviews("http://example.test", "token", 9, io.StringIO())

    def test_private_export_sets_mode_and_preserves_old_file_on_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "reviews.jsonl")
            with patch.object(EXPORTER.urllib.request, "urlopen", return_value=io.BytesIO(b"[]")):
                self.assertEqual(EXPORTER.write_private_export(path, "http://example.test", "token", 9), 0)
            self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)

            with open(path, "w", encoding="utf-8") as target:
                target.write("preserve\n")
            with patch.object(EXPORTER.urllib.request, "urlopen", side_effect=OSError("offline")):
                with self.assertRaises(OSError):
                    EXPORTER.write_private_export(path, "http://example.test", "token", 9)
            with open(path, encoding="utf-8") as source:
                self.assertEqual(source.read(), "preserve\n")


if __name__ == "__main__":
    unittest.main()
