import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error


SPEC = importlib.util.spec_from_file_location(
    "tester_inventory_export", Path(__file__).resolve().parents[1] / "script/export-tester-proposal-inventory.py",
)
EXPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORT)


def fixture():
    workspace = {"project": {"id": 9, "currentDraftVersionId": 21},
                 "version": {"id": 21, "projectId": 9, "status": "draft"}}
    workflow = {"projectId": 9, "status": "planned", "requirementText": "业务需求正文",
                "brief": {"goal": "goal"}, "answers": [], "plan": {"decisions": []},
                "testerReport": {"schemaVersion": 1, "proposals": [
                    {"id": "scenario-1", "category": "adversarial", "title": "秘密标题",
                     "rationale": "业务依据", "inputFields": ["b", "a"], "reviewRequired": True},
                    {"id": "scenario-2", "category": "required_input", "title": "另一条",
                     "rationale": "业务依据", "inputFields": [], "reviewRequired": True,
                     "testCaseId": 42},
                ]}}
    return workspace, workflow


def api_body(value):
    return io.BytesIO(json.dumps(value, ensure_ascii=False).encode())


class TesterInventoryExportTests(unittest.TestCase):
    def test_inventory_includes_converted_proposals_and_omits_business_content(self):
        rows = EXPORT.inventory_rows(*fixture(), 9)
        self.assertEqual([row["proposalId"] for row in rows], ["scenario-1", "scenario-2"])
        self.assertEqual(set(rows[0]), {"projectId", "versionId", "reportDigest", "proposalId"})
        self.assertEqual(len(rows[0]["reportDigest"]), 64)
        self.assertEqual(rows[0]["reportDigest"], rows[1]["reportDigest"])
        self.assertNotIn("秘密标题", json.dumps(rows, ensure_ascii=False))

    def test_digest_ignores_processing_state_and_proposal_order_but_tracks_business_changes(self):
        workspace, workflow = fixture()
        expected = EXPORT.inventory_rows(workspace, workflow, 9)
        changed = copy.deepcopy(workflow)
        changed["updatedAt"] = "later"
        changed["testerReport"]["proposals"].reverse()
        for item in changed["testerReport"]["proposals"]:
            item.update({"judgeReview": {"status": "suspicious"}, "testCaseId": 111,
                         "executionStatus": "passed"})
            item["inputFields"].reverse()
        self.assertEqual(EXPORT.inventory_rows(workspace, changed, 9), expected)
        changed["requirementText"] = "changed business requirement"
        self.assertNotEqual(EXPORT.inventory_rows(workspace, changed, 9), expected)
        changed = copy.deepcopy(workflow)
        changed["testerReport"]["proposals"][0]["rationale"] = "changed"
        self.assertNotEqual(EXPORT.inventory_rows(workspace, changed, 9), expected)

    def test_invalid_scope_schema_and_duplicate_proposals_are_rejected(self):
        for mutate in [
            lambda w, f: w["project"].update(id=True),
            lambda w, f: w["project"].update(currentDraftVersionId=22),
            lambda w, f: w["version"].update(status="published"),
            lambda w, f: f.update(projectId=10),
            lambda w, f: f["testerReport"].update(schemaVersion=True),
            lambda w, f: f["testerReport"]["proposals"].append(f["testerReport"]["proposals"][0]),
            lambda w, f: f["testerReport"]["proposals"][0].update(id="../bad"),
            lambda w, f: f["testerReport"]["proposals"][0].update(inputFields=[1]),
            lambda w, f: f["testerReport"]["proposals"][0].update(reviewRequired=1),
            lambda w, f: f.update(plan=None),
        ]:
            workspace, workflow = fixture()
            mutate(workspace, workflow)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                EXPORT.inventory_rows(workspace, workflow, 9)

    def test_api_export_uses_only_get_and_rechecks_snapshot(self):
        workspace, workflow = fixture()
        with patch.object(EXPORT.urllib.request, "build_opener") as build:
            build.return_value.open.side_effect = [api_body(item) for item in [workspace, workflow, workflow, workspace]]
            rows = EXPORT.fetch_inventory("https://example.test", "secret-token", 9)
            calls = build.return_value.open.call_args_list
        self.assertEqual(len(rows), 2)
        self.assertEqual([call.args[0].get_method() for call in calls], ["GET"] * 4)
        self.assertEqual([call.args[0].full_url for call in calls], [
            "https://example.test/api/projects/9", "https://example.test/api/projects/9/workflow",
            "https://example.test/api/projects/9/workflow", "https://example.test/api/projects/9",
        ])

    def test_concurrent_draft_or_business_changes_fail_instead_of_mixing_reports(self):
        for change_draft in [False, True]:
            workspace, workflow = fixture()
            after_workspace, after_workflow = copy.deepcopy(workspace), copy.deepcopy(workflow)
            if change_draft:
                after_workspace["project"]["currentDraftVersionId"] = 22
                after_workspace["version"]["id"] = 22
            else:
                after_workflow["brief"]["goal"] = "changed"
            with self.subTest(change_draft=change_draft), patch.object(EXPORT.urllib.request, "build_opener") as build:
                build.return_value.open.side_effect = [api_body(item) for item in [workspace, workflow, after_workflow, after_workspace]]
                with self.assertRaisesRegex(ValueError, "changed"):
                    EXPORT.fetch_inventory("https://example.test", "token", 9)

    def test_malformed_oversized_and_duplicate_json_or_network_error_fail_without_token(self):
        errors = [io.BytesIO(b'{"secret":1,"secret":2}'), io.BytesIO(b'{"value":NaN}'),
                  io.BytesIO(b"x" * (EXPORT.MAX_RESPONSE_BYTES + 1))]
        for body in errors:
            with patch.object(EXPORT.urllib.request, "build_opener") as build:
                build.return_value.open.return_value = body
                with self.assertRaises(ValueError):
                    EXPORT.fetch_inventory("https://example.test", "secret-token", 9)
        with patch.object(EXPORT.urllib.request, "build_opener") as build:
            build.return_value.open.side_effect = urllib.error.URLError("secret-token")
            with self.assertRaises(ValueError) as raised:
                EXPORT.fetch_inventory("https://example.test", "secret-token", 9)
        self.assertNotIn("secret-token", str(raised.exception))
        self.assertIsNone(EXPORT.NoRedirect().redirect_request(None, None, 302, "redirect", {}, "https://other.test"))

    def test_private_atomic_file_and_existing_output_preserved_on_failure(self):
        rows = EXPORT.inventory_rows(*fixture(), 9)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "inventory.jsonl"
            with patch.object(EXPORT, "fetch_inventory", return_value=rows):
                self.assertEqual(EXPORT.write_private_export(path, "base", "token", 9), 2)
            self.assertEqual(os.stat(path).st_mode & 0o777, 0o600)
            saved = path.read_bytes()
            with patch.object(EXPORT, "fetch_inventory", side_effect=ValueError("not authorized")):
                with self.assertRaises(ValueError):
                    EXPORT.write_private_export(path, "base", "token", 9)
            self.assertEqual(path.read_bytes(), saved)
            self.assertEqual(list(Path(directory).iterdir()), [path])

    def test_input_validation_prevents_network_calls(self):
        with patch.object(EXPORT.urllib.request, "build_opener") as build:
            for api_base, token, project_id in [
                ("file:///tmp/secret", "token", 9), ("https://user:password@example.test", "token", 9),
                ("https://example.test?token=x", "token", 9), ("https://example.test", "", 9),
                ("https://example.test", "token\nvalue", 9), ("https://example.test", "token", True),
            ]:
                with self.subTest(api_base=api_base), self.assertRaises(ValueError):
                    EXPORT.fetch_inventory(api_base, token, project_id)
            build.assert_not_called()


if __name__ == "__main__":
    unittest.main()
