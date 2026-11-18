import importlib.util
import json
import os
import tempfile
import unittest


SCRIPT_PATH = os.path.join(
    os.path.dirname(__file__), "..", "script", "eval-decision-explanation-reviews.py"
)
SPEC = importlib.util.spec_from_file_location("eval_explanation_reviews", SCRIPT_PATH)
EVALUATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVALUATOR)


def row(explanation_id=1, reviewer="reviewer-a", label="readable", **overrides):
    result = {"explanationId": explanation_id, "explanationRevision": 1, "reviewer": reviewer, "label": label}
    result.update(overrides)
    return result


class EvalDecisionExplanationReviewsTests(unittest.TestCase):
    def test_report_uses_explanation_cases_and_majority_vote(self):
        rows = [
            row(1, "reviewer-a", "readable"),
            row(1, "reviewer-b", "readable"),
            row(2, "reviewer-a", "needs_revision"),
            row(2, "reviewer-b", "inconclusive"),
            row(3, "reviewer-a", "readable"),
            row(3, "reviewer-b", "needs_revision"),
        ]
        report = EVALUATOR.build_report(rows, minimum_cases=1, target_rate=0.5)
        self.assertEqual(report["evaluationCount"], 6)
        self.assertEqual(report["uniqueExplanationCount"], 3)
        self.assertEqual(report["caseCounts"], {
            "reviewed": 3, "determinate": 2, "readable": 1,
            "needs_revision": 1, "inconclusive": 1,
        })
        self.assertEqual(report["readability"]["rate"], 0.5)
        self.assertTrue(report["reviewersAndExplanationIdsOmitted"])

    def test_sample_gate_distinguishes_insufficient_from_failed_target(self):
        insufficient = EVALUATOR.build_report([row()], minimum_cases=2)
        self.assertEqual(insufficient["readability"]["status"], "insufficient_samples")
        failed = EVALUATOR.build_report(
            [row(1, label="needs_revision"), row(2, label="needs_revision")],
            minimum_cases=2,
        )
        self.assertEqual(failed["readability"]["status"], "target_not_met")

    def test_duplicate_vote_and_invalid_data_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate reviewer/explanation"):
            EVALUATOR.build_report([row(), row()])
        for invalid in (row(explanation_id=True), row(label="unknown"), row(extra="payload")):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                EVALUATOR.build_report([invalid])

    def test_revisions_keep_votes_separate_and_require_explicit_identity(self):
        report = EVALUATOR.build_report([
            row(label="needs_revision", explanationRevision=1),
            row(label="readable", explanationRevision=2),
        ], minimum_cases=1)
        self.assertEqual(report["uniqueExplanationCount"], 1)
        self.assertEqual(report["uniqueExplanationRevisionCount"], 2)
        self.assertEqual(report["readability"]["determinateCases"], 1)
        self.assertEqual(report["readability"]["rate"], 1.0)
        self.assertEqual(report["supersededReviewCount"], 1)
        self.assertEqual(report["countedReviewCount"], 1)
        self.assertEqual(report["revisionPolicy"], "latest_supplied_revision_per_explanation")
        many_revisions = [row(explanationRevision=revision) for revision in range(1, 101)]
        self.assertEqual(EVALUATOR.build_report(many_revisions)["readability"]["status"], "insufficient_samples")
        for revision in (None, True, 0, -1, "1", EVALUATOR.MAX_I64 + 1):
            with self.subTest(revision=revision), self.assertRaises(ValueError):
                EVALUATOR.build_report([row(explanationRevision=revision)])
        legacy = row()
        del legacy["explanationRevision"]
        with self.assertRaisesRegex(ValueError, "missing fields: explanationRevision"):
            EVALUATOR.build_report([legacy])

    def test_wilson_interval_is_null_for_no_determinate_cases(self):
        report = EVALUATOR.build_report([row(label="inconclusive")])
        self.assertIsNone(report["readability"]["wilson95"])
        self.assertIsNone(report["readability"]["rate"])

    def test_jsonl_loader_rejects_duplicate_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "reviews.jsonl")
            with open(path, "w", encoding="utf-8") as target:
                target.write('{"explanationId":1,"explanationId":2,"reviewer":"r","label":"readable"}\n')
            with self.assertRaisesRegex(ValueError, "duplicate JSON key"):
                EVALUATOR.load_records(path)


if __name__ == "__main__":
    unittest.main()
