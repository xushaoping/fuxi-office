import importlib.util
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "script" / "extract-sse-result.py"
SPEC = importlib.util.spec_from_file_location("extract_sse_result", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ExtractSseResultTests(unittest.TestCase):
    def extract(self, content, event="workflow_result"):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "response.sse"
            path.write_text(content, encoding="utf-8")
            return MODULE.extract_result(path, event)

    def test_extracts_terminal_event_after_progress_events(self):
        result = self.extract(
            "event: stage\ndata: {\"name\":\"analyst\"}\n\n"
            "event: workflow_result\ndata: {\"status\":\"analyzed\"}\n\n"
        )
        self.assertEqual(result, {"status": "analyzed"})

    def test_rejects_error_event_even_when_terminal_event_is_missing(self):
        with self.assertRaisesRegex(ValueError, "model failed"):
            self.extract("event: error\ndata: {\"error\":\"model failed\"}\n\n")

    def test_rejects_missing_duplicate_and_invalid_terminal_events(self):
        with self.assertRaisesRegex(ValueError, "found 0"):
            self.extract("event: stage\ndata: {}\n\n")
        with self.assertRaisesRegex(ValueError, "found 2"):
            self.extract(
                "event: workflow_result\ndata: {}\n\n"
                "event: workflow_result\ndata: {}\n\n"
            )
        with self.assertRaisesRegex(ValueError, "invalid JSON"):
            self.extract("event: workflow_result\ndata: not-json\n\n")

    def test_extracts_engineer_result_event(self):
        self.assertEqual(
            self.extract("event: result\ndata: {\"ok\":true}\n\n", "result"),
            {"ok": True},
        )


if __name__ == "__main__":
    unittest.main()
