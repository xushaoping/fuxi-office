import importlib.util
import json
import os
import tempfile
import unittest
from unittest.mock import patch


SCRIPT_PATH = os.path.join(
    os.path.dirname(__file__), "..", "script", "reconcile-decision-run-outcomes.py"
)
SPEC = importlib.util.spec_from_file_location("reconcile_decision_run_outcomes", SCRIPT_PATH)
CLI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLI)


class Response:
    def __init__(self, value):
        self.body = json.dumps(value).encode()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return None

    def read(self, limit):
        return self.body[:limit]


class ReconcileDecisionRunOutcomesTests(unittest.TestCase):
    def test_input_accepts_only_bounded_unique_run_ids(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "runs.jsonl")
            with open(path, "w", encoding="utf-8") as target:
                target.write('{"runId":4}\n\n{"runId":9}\n')
            self.assertEqual(CLI.read_run_ids(path), [4, 9])
            with open(path, "w", encoding="utf-8") as target:
                target.write('{"runId":4,"privateInput":{"secret":true}}\n')
            with self.assertRaisesRegex(ValueError, "only runId"):
                CLI.read_run_ids(path)
            with open(path, "w", encoding="utf-8") as target:
                target.write('{"runId":4}\n{"runId":4}\n')
            with self.assertRaisesRegex(ValueError, "duplicates"):
                CLI.read_run_ids(path)

    def test_response_requires_request_order_and_supported_states(self):
        with self.assertRaisesRegex(ValueError, "out of request order"):
            CLI.validate_response({"results": [{"runId": 8, "state": "ready"}]}, [7])
        with self.assertRaisesRegex(ValueError, "unsupported state"):
            CLI.validate_response({"results": [{"runId": 7, "state": "maybe"}]}, [7])
        with self.assertRaisesRegex(ValueError, "unsupported state"):
            CLI.validate_response({"results": [{"runId": 7, "state": []}]}, [7])

    def test_redirects_are_not_followed_with_the_service_credential(self):
        handler = CLI.RejectRedirects()
        redirected = handler.redirect_request(None, None, 302, "Found", {}, "https://other.example")
        self.assertIsNone(redirected)

    def test_batches_send_scoped_credential_and_export_only_id_and_state(self):
        ids = list(range(1, 103))
        requests = []

        def fake_urlopen(request, timeout):
            requests.append((request, timeout))
            payload = json.loads(request.data)
            return Response({
                "results": [
                    {
                        "runId": run_id,
                        "state": "already_reported" if run_id == 1 else "ready",
                        "businessReference": "private-reference",
                        "assessment": "mismatch",
                    }
                    for run_id in payload["runIds"]
                ]
            })

        with patch.object(CLI, "open_url", side_effect=fake_urlopen):
            rows = CLI.reconcile("https://api.example", "service-secret", 12, "svc-a", ids)
        self.assertEqual(len(requests), 2)
        self.assertEqual([len(json.loads(req.data)["runIds"]) for req, _ in requests], [100, 2])
        first_request, timeout = requests[0]
        self.assertEqual(first_request.get_header("X-decision-key"), "service-secret")
        self.assertEqual(json.loads(first_request.data)["serviceId"], "svc-a")
        self.assertEqual(timeout, 30)
        self.assertEqual(rows[0], {"runId": 1, "state": "already_reported"})
        self.assertNotIn("businessReference", json.dumps(rows))
        self.assertNotIn("private-reference", json.dumps(rows))

    def test_output_is_private_and_failed_request_preserves_previous_file(self):
        with tempfile.TemporaryDirectory() as directory:
            input_path = os.path.join(directory, "runs.jsonl")
            output_path = os.path.join(directory, "results.jsonl")
            with open(input_path, "w", encoding="utf-8") as target:
                target.write('{"runId":7}\n')
            with patch.object(CLI, "open_url", return_value=Response({
                "results": [{"runId": 7, "state": "ready"}]
            })):
                self.assertEqual(
                    CLI.write_reconciliation(
                        "http://localhost", "secret", 2, "svc", [7], output_path
                    ),
                    1,
                )
            self.assertEqual(os.stat(output_path).st_mode & 0o777, 0o600)
            with open(output_path, encoding="utf-8") as source:
                self.assertEqual(json.loads(source.readline()), {"runId": 7, "state": "ready"})

            with open(output_path, "w", encoding="utf-8") as target:
                target.write("previous\n")
            with patch.object(CLI, "open_url", return_value=Response({"results": []})):
                with self.assertRaisesRegex(ValueError, "count does not match"):
                    CLI.write_reconciliation(
                        "http://localhost", "secret", 2, "svc", [7], output_path
                    )
            with open(output_path, encoding="utf-8") as source:
                self.assertEqual(source.read(), "previous\n")


if __name__ == "__main__":
    unittest.main()
