import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT_PATH = Path(__file__).resolve().parents[1] / "script" / "eval-tester-proposal-reviews.py"
SPEC = importlib.util.spec_from_file_location("eval_tester_proposal_reviews", SCRIPT_PATH)
EVALUATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVALUATOR)


def candidate(proposal_id="proposal-1", **overrides):
    result = {"projectId": 10, "versionId": 20, "reportDigest": "a" * 64, "proposalId": proposal_id}
    result.update(overrides)
    return result


def review(proposal_id="proposal-1", reviewer="reviewer-a", label="accepted", **overrides):
    return {**candidate(proposal_id), "reviewer": reviewer, "label": label, **overrides}


class EvalTesterProposalReviewsTests(unittest.TestCase):
    def test_inventory_denominator_separates_unreviewed_ties_and_abstentions(self):
        inventory = [candidate(str(index)) for index in range(1, 6)]
        reviews = [
            review("1", "a", "accepted"), review("1", "b", "accepted"),
            review("1", "c", "rejected"), review("2", "a", "rejected"),
            review("2", "b", "inconclusive"), review("3", "a", "accepted"),
            review("3", "b", "rejected"), review("4", "a", "inconclusive"),
        ]
        report = EVALUATOR.build_report(inventory, reviews, minimum_cases=1)
        self.assertEqual(report["reviewCount"], 8)
        self.assertEqual(report["caseCounts"], {
            "candidate": 5, "reviewed": 4, "unreviewed": 1,
            "determinate": 2, "accepted": 1, "rejected": 1,
            "inconclusive": 2, "tied": 1, "onlyInconclusive": 1,
        })
        self.assertEqual(report["coverage"]["reviewedRate"], 0.8)
        self.assertEqual(report["coverage"]["determinateRate"], 0.4)
        self.assertEqual(report["acceptance"]["rate"], 0.5)
        self.assertEqual(report["acceptance"]["acceptedFractionOfAllCandidates"], 0.2)
        self.assertEqual(report["acceptance"]["status"], "insufficient_samples")
        self.assertEqual(report["acceptance"]["insufficiencyReasons"], ["minimum_determinate_coverage"])

    def test_selected_positive_reviews_do_not_pass_with_missing_inventory_reviews(self):
        inventory = [candidate(str(index)) for index in range(200)]
        selected = [review(str(index)) for index in range(100)]
        report = EVALUATOR.build_report(inventory, selected)
        self.assertEqual(report["acceptance"]["rate"], 1.0)
        self.assertEqual(report["caseCounts"]["unreviewed"], 100)
        self.assertEqual(report["acceptance"]["status"], "insufficient_samples")

    def test_inconclusive_votes_cannot_be_used_to_satisfy_effective_coverage(self):
        inventory = [candidate(str(index)) for index in range(200)]
        reviews = [review(str(index), label="accepted" if index < 100 else "inconclusive") for index in range(200)]
        report = EVALUATOR.build_report(inventory, reviews)
        self.assertEqual(report["coverage"]["reviewedRate"], 1.0)
        self.assertEqual(report["coverage"]["determinateRate"], 0.5)
        self.assertEqual(report["acceptance"]["status"], "insufficient_samples")

    def test_default_target_and_minimum_case_boundaries(self):
        inventory = [candidate(str(index)) for index in range(100)]
        for accepted, expected in [(70, "target_met"), (69, "target_not_met")]:
            reviews = [review(str(index), label="accepted" if index < accepted else "rejected") for index in range(100)]
            report = EVALUATOR.build_report(inventory, reviews)
            self.assertEqual(report["acceptance"]["status"], expected)
        report = EVALUATOR.build_report(inventory[:99], [review(str(index)) for index in range(99)])
        self.assertEqual(report["acceptance"]["status"], "insufficient_samples")
        self.assertEqual(report["acceptance"]["insufficiencyReasons"], ["minimum_determinate_cases"])

    def test_coverage_threshold_checks_unrounded_values(self):
        inventory = [candidate(str(index)) for index in range(10)]
        for number, expected in [(9, "target_met"), (8, "insufficient_samples")]:
            report = EVALUATOR.build_report(inventory, [review(str(index)) for index in range(number)], minimum_cases=1)
            self.assertEqual(report["acceptance"]["status"], expected)

    def test_case_identity_includes_project_version_and_report(self):
        inventory = [candidate(), candidate(projectId=11), candidate(versionId=21), candidate(reportDigest="b" * 64)]
        rows = [{**item, "reviewer": "same-reviewer", "label": "accepted"} for item in inventory]
        report = EVALUATOR.build_report(inventory, rows, minimum_cases=1)
        self.assertEqual(report["caseCounts"]["accepted"], 4)
        self.assertEqual(report["acceptance"]["status"], "target_met")

    def test_duplicate_inventory_and_votes_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate inventory"):
            EVALUATOR.build_report([candidate(), candidate()], [])
        for second_vote in (review(), review(label="rejected")):
            with self.subTest(second_vote=second_vote), self.assertRaisesRegex(ValueError, "duplicate reviewer/case"):
                EVALUATOR.build_report([candidate()], [review(), second_vote])

    def test_cross_inventory_and_wrong_report_references_are_rejected(self):
        for foreign in (review("other"), review(projectId=11), review(versionId=21), review(reportDigest="b" * 64)):
            with self.subTest(foreign=foreign), self.assertRaisesRegex(ValueError, "absent from"):
                EVALUATOR.build_report([candidate()], [foreign])

    def test_invalid_inventory_fields_and_integer_bool_are_rejected(self):
        for row in (
            [], None, candidate(projectId=True), candidate(versionId=0), candidate(projectId=-1),
            candidate(versionId=EVALUATOR.MAX_I64 + 1), candidate(projectId=1.0),
            candidate(reportDigest="A" * 64), candidate(reportDigest="a" * 63),
            candidate(proposalId=" "), candidate(proposalId="x" * 65), candidate(proposalId={}),
            candidate(extra="private input"), {key: value for key, value in candidate().items() if key != "reportDigest"},
        ):
            with self.subTest(row=row), self.assertRaises(ValueError):
                EVALUATOR.build_report([row], [])

    def test_invalid_review_fields_and_labels_are_rejected(self):
        for row in (
            review(label="readable"), review(label=[]), review(label=True), review(reviewer=""),
            review(reviewer=" a"), review(reviewer="a "), review(reviewer="a\n"),
            review(reviewer="a\x7f"), review(reviewer=1), review(reviewer="评" * 86),
            review(extra="private expected result"),
        ):
            with self.subTest(row=row), self.assertRaises(ValueError):
                EVALUATOR.build_report([candidate()], [row])

    def test_empty_reviews_and_only_inconclusive_samples_do_not_fabricate_rate(self):
        for rows in ([], [review(label="inconclusive")]):
            report = EVALUATOR.build_report([candidate()], rows)
            self.assertIsNone(report["acceptance"]["rate"])
            self.assertIsNone(report["acceptance"]["wilson95"])
            self.assertEqual(report["acceptance"]["status"], "insufficient_samples")
        with self.assertRaisesRegex(ValueError, "must not be empty"):
            EVALUATOR.build_report([], [])

    def test_invalid_thresholds_are_rejected(self):
        for bad in (True, 0, -1, 1.5, EVALUATOR.MAX_RECORDS + 1):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                EVALUATOR.build_report([candidate()], [], minimum_cases=bad)
        for name in ("target_rate", "minimum_coverage"):
            for bad in (True, "0.7", -0.1, 1.1, float("nan"), float("inf")):
                with self.subTest(name=name, bad=bad), self.assertRaises(ValueError):
                    EVALUATOR.build_report([candidate()], [], **{name: bad})

    def test_wilson_interval_known_vector_and_extremes(self):
        interval = EVALUATOR.wilson_interval(70, 100)
        self.assertEqual(interval, {"lower": 0.604151, "upper": 0.781051})
        self.assertEqual(EVALUATOR.wilson_interval(0, 100)["lower"], 0.0)
        self.assertEqual(EVALUATOR.wilson_interval(100, 100)["upper"], 1.0)

    def test_report_omits_case_digests_ids_and_reviewers(self):
        item = candidate("secret-proposal", projectId=987654321, versionId=123456789)
        report = EVALUATOR.build_report([item], [{**item, "reviewer": "secret-reviewer", "label": "accepted"}])
        rendered = json.dumps(report)
        for value in ("secret-proposal", "secret-reviewer", "a" * 64, "987654321", "123456789"):
            self.assertNotIn(value, rendered)
        self.assertTrue(report["reviewersAndCaseIdsOmitted"])

    def test_loader_rejects_duplicate_keys_nonfinite_and_invalid_json(self):
        for text in (
            '{"projectId":1,"projectId":2}',
            json.dumps(candidate()).replace('"projectId": 10', '"projectId": NaN'),
            '{"broken":', '[]',
        ):
            with self.subTest(text=text), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "bad.jsonl"
                path.write_text(text + "\n")
                with self.assertRaises(ValueError):
                    EVALUATOR.load_records(path)

    def test_loader_binds_utf8_size_file_size_and_record_count(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.jsonl"
            path.write_bytes(b"\xff\n")
            with self.assertRaisesRegex(ValueError, "invalid JSONL"):
                EVALUATOR.load_records(path)
            path.write_text("#" + "评" * 100 + "\n")
            with patch.object(EVALUATOR, "MAX_LINE_BYTES", 100), self.assertRaisesRegex(ValueError, "exceeds 100 bytes"):
                EVALUATOR.load_records(path)
            path.write_text("# comment\n" * 4)
            with patch.object(EVALUATOR, "MAX_FILE_BYTES", 20), self.assertRaisesRegex(ValueError, "input exceeds 20 bytes"):
                EVALUATOR.load_records(path)
            path.write_text(json.dumps(candidate()) + "\n" + json.dumps(candidate("p2")) + "\n")
            with patch.object(EVALUATOR, "MAX_RECORDS", 1), self.assertRaisesRegex(ValueError, "exceeds 1 records"):
                EVALUATOR.load_records(path)

    def test_programmatic_input_record_limits_are_enforced(self):
        with patch.object(EVALUATOR, "MAX_RECORDS", 1), self.assertRaisesRegex(ValueError, "exceeds 1 records"):
            EVALUATOR.build_report([candidate(), candidate("p2")], [], minimum_cases=1)
        with patch.object(EVALUATOR, "MAX_RECORDS", 1), self.assertRaisesRegex(ValueError, "exceeds 1 records"):
            EVALUATOR.build_report([candidate()], [review(), review(reviewer="another")], minimum_cases=1)

    def test_cli_reads_two_files_and_emits_only_report_json(self):
        with tempfile.TemporaryDirectory() as directory:
            inventory_path = Path(directory) / "inventory.jsonl"
            review_path = Path(directory) / "reviews.jsonl"
            inventory_path.write_text("# Frozen before review\n\n" + json.dumps(candidate()) + "\n")
            review_path.write_text(json.dumps(review()) + "\n")
            result = subprocess.run([sys.executable, str(SCRIPT_PATH), str(inventory_path), str(review_path)], text=True, capture_output=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, "")
            report = json.loads(result.stdout)
            self.assertEqual(report["acceptance"]["status"], "insufficient_samples")

    def test_cli_rejects_bad_input_without_partial_report(self):
        with tempfile.TemporaryDirectory() as directory:
            inventory_path = Path(directory) / "inventory.jsonl"
            review_path = Path(directory) / "reviews.jsonl"
            inventory_path.write_text(json.dumps(candidate()) + "\n")
            review_path.write_text(json.dumps(review("outside-inventory")) + "\n")
            result = subprocess.run([sys.executable, str(SCRIPT_PATH), str(inventory_path), str(review_path)], text=True, capture_output=True, check=False)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, "")
            self.assertIn("absent from", result.stderr)

    def test_main_rejects_nan_cli_threshold(self):
        with tempfile.TemporaryDirectory() as directory:
            inventory_path = Path(directory) / "inventory.jsonl"
            review_path = Path(directory) / "reviews.jsonl"
            inventory_path.write_text(json.dumps(candidate()) + "\n")
            review_path.write_text("")
            output, errors = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                status = EVALUATOR.main([str(inventory_path), str(review_path), "--target-rate", "nan"])
            self.assertEqual(status, 1)
            self.assertEqual(output.getvalue(), "")
            self.assertIn("finite ratio", errors.getvalue())


if __name__ == "__main__":
    unittest.main()
