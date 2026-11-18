import importlib.util
import io
import json
import os
import tempfile
import unittest
from unittest.mock import patch


SCRIPT_PATH = os.path.join(
    os.path.dirname(__file__), "..", "script", "eval-agent-orchestrations.py"
)
SPEC = importlib.util.spec_from_file_location("eval_agent_orchestrations", SCRIPT_PATH)
EVALUATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVALUATOR)


def step(step_id, step_no, attempt_no, status, usage=None, orchestration_id=22, model="model-a", agent_role="engineer"):
    return {
        "id": step_id,
        "orchestrationId": orchestration_id,
        "stepNo": step_no,
        "attemptNo": attempt_no,
        "agentRole": agent_role,
        "taskKind": "generate_dmn_dsl",
        "model": model,
        "status": status,
        "usage": usage,
        "error": {"message": "secret provider error"},
        "artifactDigest": "secret artifact digest",
    }


def orchestration(orchestration_id, steps):
    return {
        "id": orchestration_id,
        "parentOrchestrationId": None,
        "retryOfOrchestrationId": None,
        "triggerKind": "workflow",
        "status": "succeeded",
        "currentStage": "tester",
        "createdAt": "2026-11-14T08:00:00Z",
        "finishedAt": "2026-11-14T08:00:05Z",
        "steps": steps,
    }


class Response:
    def __init__(self, payload):
        self.body = json.dumps(payload).encode()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return None

    def read(self, limit):
        return self.body[:limit]


class EvalAgentOrchestrationsTests(unittest.TestCase):
    def test_role_flow_aggregates_parent_child_tree_and_keeps_facilitator_conditional(self):
        analyst = orchestration(30, [
            step(1, 1, 1, "succeeded", orchestration_id=30, agent_role="analyst"),
        ])
        architect = orchestration(31, [
            step(2, 1, 1, "succeeded", orchestration_id=31, agent_role="architect"),
            step(3, 4, 1, "succeeded", orchestration_id=31, agent_role="tester"),
        ])
        architect["parentOrchestrationId"] = 30
        engineer = orchestration(32, [
            step(4, 1, 1, "succeeded", orchestration_id=32, agent_role="engineer"),
            step(5, 2, 1, "succeeded", orchestration_id=32, agent_role="tester"),
        ])
        engineer["parentOrchestrationId"] = 31

        role_flow = EVALUATOR.evaluate([engineer, architect, analyst])["roleFlow"]

        self.assertEqual(role_flow["workflowTreeCount"], 1)
        self.assertEqual(role_flow["completeCoreRoleTrees"], 1)
        self.assertEqual(role_flow["coreRoleCompletion"]["value"], 1.0)
        self.assertEqual(role_flow["roles"]["facilitator"]["attemptedTrees"], 0)
        self.assertEqual(role_flow["roles"]["tester"]["succeededTrees"], 1)

    def test_role_flow_counts_partial_and_failed_roles_without_joining_unrelated_trees(self):
        analyst = orchestration(30, [
            step(1, 1, 1, "succeeded", orchestration_id=30, agent_role="analyst"),
        ])
        architect = orchestration(31, [
            step(2, 1, 1, "succeeded", orchestration_id=31, agent_role="architect"),
            step(3, 4, 1, "failed", orchestration_id=31, agent_role="tester"),
        ])
        architect["parentOrchestrationId"] = 30
        unrelated = orchestration(29, [
            step(4, 1, 1, "succeeded", orchestration_id=29, agent_role="tester"),
        ])

        role_flow = EVALUATOR.evaluate([architect, analyst, unrelated])["roleFlow"]

        self.assertEqual(role_flow["workflowTreeCount"], 1)
        self.assertEqual(role_flow["completeCoreRoleTrees"], 0)
        self.assertEqual(role_flow["coreRoleCompletion"]["value"], 0.0)
        self.assertEqual(role_flow["roles"]["tester"]["attemptedTrees"], 1)
        self.assertEqual(role_flow["roles"]["tester"]["succeededTrees"], 0)

    def test_orchestration_success_uses_records_not_step_successes(self):
        rows = []
        for index, status in enumerate(("succeeded", "failed", "cancelled", "pending", "running")):
            item_id = 30 - index
            item = orchestration(item_id, [step(index + 1, 1, 1, "succeeded", orchestration_id=item_id)])
            item["status"] = status
            rows.append(item)
        summary = EVALUATOR.evaluate(rows, minimum_cases=2)["orchestrationAttempts"]
        self.assertEqual(summary["success"]["value"], 0.5)
        self.assertEqual(summary["success"]["denominator"], 2)
        self.assertEqual(summary["success"]["status"], "target_not_met")
        self.assertEqual(summary["success"]["targetRate"], 0.95)
        self.assertEqual(summary["inFlightCount"], 2)
        self.assertEqual(summary["statusCounts"]["cancelled"], 1)
        self.assertEqual(EVALUATOR.evaluate(rows)["orchestrationAttempts"]["success"]["status"], "insufficient_samples")

    def test_success_target_threshold_and_default_minimum_are_explicit(self):
        rows = []
        for item_id in range(100, 0, -1):
            item = orchestration(item_id, [])
            item["status"] = "succeeded" if item_id > 5 else "failed"
            rows.append(item)
        success = EVALUATOR.evaluate(rows)["orchestrationAttempts"]["success"]
        self.assertEqual(success["value"], 0.95)
        self.assertEqual(success["status"], "target_met")
        self.assertEqual(success["minimumCases"], 100)
        self.assertIsNotNone(success["wilson95"])
        rows[0]["status"] = "failed"
        self.assertEqual(EVALUATOR.evaluate(rows)["orchestrationAttempts"]["success"]["status"], "target_not_met")

    def test_retry_family_and_tree_budget_include_original_failed_usage(self):
        first = orchestration(22, [step(1, 1, 1, "failed", {"total_tokens": 40})])
        first["status"] = "failed"
        retry = orchestration(23, [step(2, 1, 2, "succeeded", {"total_tokens": 60}, orchestration_id=23)])
        retry["retryOfOrchestrationId"] = 22
        report = EVALUATOR.evaluate([retry, first], token_budget=100, minimum_cases=1)
        family = report["retryFamilies"]
        self.assertEqual(family["familyCount"], 1)
        self.assertEqual(family["retriedFamilyCount"], 1)
        self.assertEqual(family["firstAttempt"]["success"]["value"], 0.0)
        self.assertEqual(family["latestAttempt"]["success"]["value"], 1.0)
        self.assertEqual(report["orchestrationAttempts"]["success"]["value"], 0.5)
        budget = report["budgetTrees"]
        self.assertEqual(budget["treeCount"], 1)
        self.assertEqual(budget["observedTokenTotal"], 100)
        self.assertEqual(budget["completedWithinBudget"]["status"], "target_met")
        self.assertEqual(budget["completedWithinBudget"]["value"], 1.0)
        too_small = EVALUATOR.evaluate([retry, first], token_budget=99, minimum_cases=1)["budgetTrees"]
        self.assertEqual(too_small["overBudgetTrees"], 1)
        self.assertEqual(too_small["completedWithinBudget"]["status"], "target_not_met")

    def test_budget_trees_use_cumulative_primary_usage_and_keep_child_family_separate(self):
        parent = orchestration(22, [
            step(1, 1, 1, "succeeded", {"total_tokens": 80}),
            step(2, 2, 1, "succeeded", None),
            step(3, 3, 1, "succeeded", None, model=None),
        ])
        child = orchestration(23, [step(4, 1, 1, "succeeded", {"total_tokens": 20}, orchestration_id=23)])
        child["parentOrchestrationId"] = 22
        report = EVALUATOR.evaluate([child, parent], token_budget=100, minimum_cases=1)
        self.assertEqual(report["retryFamilies"]["familyCount"], 2)
        budget = report["budgetTrees"]
        self.assertEqual(budget["treeCount"], 1)
        self.assertEqual(budget["tokenObservedTrees"], 1)
        self.assertEqual(budget["observedTokenTotal"], 100)
        self.assertEqual(budget["completedWithinBudget"]["successes"], 1)

    def test_missing_partial_and_reservation_usage_never_count_as_zero(self):
        for usage in (None, {}, {"prompt_tokens": 8}, {"total_tokens": 0, "budget_reservation": True}):
            with self.subTest(usage=usage):
                row = orchestration(22, [step(1, 1, 1, "succeeded", usage)])
                budget = EVALUATOR.evaluate([row], token_budget=1, minimum_cases=1)["budgetTrees"]
                self.assertEqual(budget["tokenObservedTrees"], 0)
                self.assertEqual(budget["tokenMissingTrees"], 1)
                self.assertEqual(budget["tokenCoverage"]["value"], 0.0)
                self.assertIsNone(budget["completedWithinBudget"]["value"])
                self.assertEqual(budget["completedWithinBudget"]["status"], "incomplete_usage")
        zero = orchestration(22, [step(1, 1, 1, "succeeded", {"total_tokens": 0})])
        budget = EVALUATOR.evaluate([zero], token_budget=1, minimum_cases=1)["budgetTrees"]
        self.assertEqual(budget["tokenObservedTrees"], 1)
        self.assertEqual(budget["completedWithinBudget"]["status"], "target_met")

    def test_missing_usage_preserves_coverage_denominator_and_blocks_acceptance(self):
        rows = [
            orchestration(22, [step(1, 1, 1, "succeeded", {"total_tokens": 10})]),
            orchestration(21, [step(2, 1, 1, "succeeded", None, orchestration_id=21)]),
        ]
        budget = EVALUATOR.evaluate(rows, token_budget=10, minimum_cases=1)["budgetTrees"]
        self.assertEqual(budget["evaluableTrees"], 2)
        self.assertEqual(budget["tokenCoverage"]["value"], 0.5)
        self.assertEqual(budget["completedWithinBudget"]["value"], 1.0)
        self.assertEqual(budget["completedWithinBudgetLowerBound"]["value"], 0.5)
        self.assertEqual(budget["completedWithinBudget"]["status"], "incomplete_usage")

    def test_inflight_cancelled_and_failed_budget_trees_are_distinct(self):
        parent = orchestration(22, [step(1, 1, 1, "succeeded", {"total_tokens": 5})])
        child = orchestration(23, [step(2, 1, 1, "running", {"total_tokens": 0}, orchestration_id=23)])
        child["parentOrchestrationId"] = 22
        child["status"] = "running"
        cancelled = orchestration(21, [step(3, 1, 1, "cancelled", {"total_tokens": 5}, orchestration_id=21)])
        cancelled["status"] = "cancelled"
        failed = orchestration(20, [step(4, 1, 1, "failed", {"total_tokens": 5}, orchestration_id=20)])
        failed["status"] = "failed"
        budget = EVALUATOR.evaluate([child, parent, cancelled, failed], token_budget=10, minimum_cases=1)["budgetTrees"]
        self.assertEqual(budget["statusCounts"], {"succeeded": 0, "failed": 1, "cancelled": 1, "inFlight": 1})
        self.assertEqual(budget["withinBudgetTrees"], 1)
        self.assertEqual(budget["completedWithinBudget"]["successes"], 0)
        self.assertEqual(budget["completedWithinBudget"]["status"], "target_not_met")

    def test_budget_target_threshold_is_ninety_percent_of_complete_trees(self):
        rows = []
        for item_id in range(100, 0, -1):
            row = orchestration(item_id, [step(item_id, 1, 1, "succeeded", {"total_tokens": 10 if item_id > 10 else 11}, orchestration_id=item_id)])
            rows.append(row)
        budget = EVALUATOR.evaluate(rows, token_budget=10)["budgetTrees"]
        self.assertEqual(budget["completedWithinBudget"]["targetRate"], 0.90)
        self.assertEqual(budget["completedWithinBudget"]["value"], 0.90)
        self.assertEqual(budget["completedWithinBudget"]["status"], "target_met")
        self.assertEqual(budget["overBudgetTrees"], 10)

    def test_budget_requires_explicit_threshold_and_valid_options(self):
        report = EVALUATOR.evaluate([])
        self.assertEqual(report["budgetTrees"]["completedWithinBudget"]["status"], "not_configured")
        self.assertIsNone(report["budgetTrees"]["withinBudgetTrees"])
        self.assertIsNone(report["budgetTrees"]["completedWithinBudgetLowerBound"])
        self.assertEqual(report["orchestrationAttempts"]["success"]["status"], "insufficient_samples")
        for options in ({"token_budget": True}, {"token_budget": 0}, {"minimum_cases": True}, {"minimum_cases": 0}):
            with self.subTest(options=options), self.assertRaises(ValueError):
                EVALUATOR.evaluate([], **options)

    def test_usage_validation_rejects_overflow_and_inconsistent_totals(self):
        for usage in (
            {"prompt_tokens": EVALUATOR.MAX_I64, "completion_tokens": 1},
            {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 5},
            {"total_tokens": 2, "budget_reservation": "true"},
        ):
            with self.subTest(usage=usage), self.assertRaises(ValueError):
                EVALUATOR.evaluate([orchestration(22, [step(1, 1, 1, "succeeded", usage)])])

    def test_long_parent_and_retry_chains_do_not_use_python_recursion(self):
        for reference_key in ("parentOrchestrationId", "retryOfOrchestrationId"):
            with self.subTest(reference_key=reference_key):
                rows = []
                for item_id in range(2500, 0, -1):
                    row = orchestration(item_id, [])
                    if item_id > 1:
                        row[reference_key] = item_id - 1
                    rows.append(row)
                report = EVALUATOR.evaluate(rows)
                self.assertEqual(report["orchestrationCount"], 2500)
                self.assertEqual(report["budgetTrees"]["treeCount"], 1)
                expected = 1 if reference_key == "retryOfOrchestrationId" else 2500
                self.assertEqual(report["retryFamilies"]["familyCount"], expected)

    def test_lineage_rejects_missing_cycles_and_ambiguous_budget_roots(self):
        missing = orchestration(22, [])
        missing["parentOrchestrationId"] = 21
        with self.assertRaisesRegex(ValueError, "missing"):
            EVALUATOR.evaluate([missing])
        older = orchestration(21, [])
        older["retryOfOrchestrationId"] = 22
        with self.assertRaisesRegex(ValueError, "cycle"):
            EVALUATOR.evaluate([missing, older])
        join = orchestration(23, [])
        join["parentOrchestrationId"] = 22
        join["retryOfOrchestrationId"] = 21
        with self.assertRaisesRegex(ValueError, "multiple budget roots"):
            EVALUATOR.evaluate([join, orchestration(22, []), orchestration(21, [])])

    def test_cli_passes_budget_options_and_validates_before_fetch(self):
        row = orchestration(22, [step(1, 1, 1, "succeeded", {"total_tokens": 5})])
        with patch.object(EVALUATOR, "fetch_history", return_value=[row]) as fetch, patch("sys.stdout", new_callable=io.StringIO) as output, patch("sys.stderr", new_callable=io.StringIO):
            self.assertEqual(EVALUATOR.main(["7", "--token", "fixture", "--token-budget", "5", "--minimum-cases", "1"]), 0)
            report = json.loads(output.getvalue())
            self.assertEqual(report["budgetTrees"]["completedWithinBudget"]["status"], "target_met")
            self.assertEqual(fetch.call_count, 1)
        with patch.object(EVALUATOR, "fetch_history") as fetch, patch("sys.stderr", new_callable=io.StringIO):
            self.assertEqual(EVALUATOR.main(["7", "--token", "fixture", "--token-budget", "0"]), 1)
            fetch.assert_not_called()

    def test_aggregates_first_pass_retries_and_observed_tokens_without_raw_fields(self):
        report = EVALUATOR.evaluate([
            orchestration(22, [
                step(1, 1, 1, "failed", {"prompt_tokens": 10, "completion_tokens": 4, "total_tokens": 14}),
                step(2, 1, 2, "succeeded", {"prompt_tokens": 11, "completion_tokens": 5, "total_tokens": 16}),
                step(3, 2, 1, "succeeded", {"prompt_tokens": 8, "completion_tokens": 2, "total_tokens": 10}),
            ]),
            orchestration(19, [step(4, 1, 1, "succeeded", None, orchestration_id=19)]),
        ])
        task = report["taskKinds"]["generate_dmn_dsl"]
        self.assertEqual(report["orchestrationCount"], 2)
        self.assertEqual(report["stepAttemptCount"], 4)
        self.assertEqual(report["retryAttemptCount"], 1)
        self.assertEqual(task["logicalSteps"], 3)
        self.assertEqual(task["firstAttemptEvaluableSteps"], 3)
        self.assertEqual(task["firstAttemptSucceeded"], 2)
        self.assertEqual(task["firstAttemptSuccessRate"], 0.6667)
        self.assertEqual(task["finalAttemptSucceeded"], 3)
        self.assertEqual(task["tokenTotal"], 40)
        self.assertEqual(task["tokenObservedAttempts"], 3)
        rendered = json.dumps(report)
        self.assertNotIn("secret provider error", rendered)
        self.assertNotIn("secret artifact digest", rendered)

    def test_validation_rejects_bad_order_usage_and_attempt_gaps(self):
        with self.assertRaisesRegex(ValueError, "descending"):
            EVALUATOR.validate_history([orchestration(11, []), orchestration(12, [])])
        invalid_usage = orchestration(22, [step(1, 1, 1, "succeeded", {"total_tokens": True})])
        with self.assertRaisesRegex(ValueError, "total_tokens"):
            EVALUATOR.evaluate([invalid_usage])
        gap = orchestration(22, [step(1, 1, 1, "failed"), step(2, 1, 3, "succeeded")])
        with self.assertRaisesRegex(ValueError, "contiguous"):
            EVALUATOR.evaluate([gap])
        duplicate_step_id = orchestration(22, [step(1, 1, 1, "failed"), step(1, 2, 1, "succeeded")])
        with self.assertRaisesRegex(ValueError, "duplicate step ID"):
            EVALUATOR.evaluate([duplicate_step_id])

    def test_retry_lineage_counts_new_orchestration_as_a_later_attempt(self):
        first = orchestration(22, [step(1, 1, 1, "failed", model="model-a")])
        retried = orchestration(23, [step(2, 1, 1, "succeeded", orchestration_id=23, model="model-b")])
        retried["retryOfOrchestrationId"] = 22
        report = EVALUATOR.evaluate([retried, first])
        task = report["taskKinds"]["generate_dmn_dsl"]
        self.assertEqual(task["logicalSteps"], 1)
        self.assertEqual(task["firstAttemptFailed"], 1)
        self.assertEqual(task["firstAttemptSuccessRate"], 0.0)
        self.assertEqual(task["finalAttemptSucceeded"], 1)
        self.assertEqual(task["retryAttempts"], 1)
        self.assertEqual(report["models"]["model-a"]["firstAttemptFailed"], 1)
        self.assertEqual(report["models"]["model-a"]["retryAttempts"], 0)
        self.assertEqual(report["models"]["model-b"]["retryAttempts"], 1)
        self.assertEqual(report["models"]["model-b"]["finalAttemptSucceeded"], 1)

    def test_model_slices_normalize_missing_names_and_keep_token_usage_scoped(self):
        missing_model = step(1, 1, 1, "succeeded", {"total_tokens": 9}, model=None)
        blank_model = step(2, 1, 1, "failed", {"total_tokens": 3}, orchestration_id=21, model=" ")
        report = EVALUATOR.evaluate([
            orchestration(22, [missing_model]),
            orchestration(21, [blank_model]),
        ])
        unknown = report["models"]["unknown"]
        self.assertEqual(unknown["attempts"], 2)
        self.assertEqual(unknown["tokenTotal"], 12)
        self.assertEqual(unknown["firstAttemptSucceeded"], 1)
        self.assertEqual(unknown["firstAttemptFailed"], 1)
        self.assertEqual(unknown["firstAttemptSuccessRate"], 0.5)
        self.assertEqual(unknown["finalAttemptFailed"], 1)

    def test_wilson_intervals_cover_empty_small_and_perfect_samples(self):
        self.assertIsNone(EVALUATOR.wilson_interval(0, 0))
        self.assertEqual(EVALUATOR.wilson_interval(1, 1), {
            "lower": 0.2065,
            "upper": 1.0,
        })
        self.assertEqual(EVALUATOR.wilson_interval(0, 1), {
            "lower": 0.0,
            "upper": 0.7935,
        })
        self.assertEqual(EVALUATOR.rate_summary(0, 0), {
            "successes": 0,
            "denominator": 0,
            "value": None,
            "wilson95": None,
        })
        with self.assertRaisesRegex(ValueError, "Wilson interval"):
            EVALUATOR.wilson_interval(2, 1)

    def test_task_and_model_rate_intervals_preserve_evaluable_denominators(self):
        root = orchestration(22, [step(1, 1, 1, "failed", model="model-a")])
        retry = orchestration(23, [step(2, 1, 1, "succeeded", orchestration_id=23, model="model-b")])
        retry["retryOfOrchestrationId"] = 22
        cancelled = orchestration(21, [step(3, 1, 1, "cancelled", orchestration_id=21)])
        report = EVALUATOR.evaluate([retry, root, cancelled])
        task = report["taskKinds"]["generate_dmn_dsl"]
        self.assertEqual(task["firstAttemptSuccess"], {
            "successes": 0,
            "denominator": 1,
            "value": 0.0,
            "wilson95": {"lower": 0.0, "upper": 0.7935},
        })
        self.assertEqual(task["firstAttemptFailure"]["denominator"], 1)
        self.assertEqual(task["finalAttemptSuccess"]["denominator"], 1)
        self.assertEqual(task["finalAttemptCancelled"], 1)
        self.assertEqual(report["models"]["model-a"]["firstAttemptFailure"]["denominator"], 1)
        self.assertEqual(report["models"]["model-b"]["finalAttemptSuccess"]["successes"], 1)
        self.assertEqual(report["models"]["model-b"]["finalAttemptSuccess"]["denominator"], 1)

        only_cancelled = EVALUATOR.evaluate([cancelled])["taskKinds"]["generate_dmn_dsl"]
        self.assertEqual(only_cancelled["firstAttemptSuccess"]["denominator"], 0)
        self.assertIsNone(only_cancelled["firstAttemptSuccess"]["wilson95"])

    def test_validation_rejects_invalid_model_labels(self):
        for model in ([], "m" * 256):
            with self.subTest(model=type(model).__name__):
                invalid = orchestration(22, [step(1, 1, 1, "succeeded", model=model)])
                with self.assertRaisesRegex(ValueError, "model"):
                    EVALUATOR.evaluate([invalid])

    def test_cancelled_attempts_are_reported_but_excluded_from_success_rates(self):
        cancelled = orchestration(22, [step(1, 1, 1, "cancelled")])
        failed = orchestration(21, [step(2, 1, 1, "failed", orchestration_id=21)])
        succeeded = orchestration(20, [step(3, 1, 1, "succeeded", orchestration_id=20)])
        task = EVALUATOR.evaluate([cancelled, failed, succeeded])["taskKinds"]["generate_dmn_dsl"]
        self.assertEqual(task["firstAttemptEvaluableSteps"], 2)
        self.assertEqual(task["firstAttemptCancelled"], 1)
        self.assertEqual(task["firstAttemptSuccessRate"], 0.5)
        self.assertEqual(task["finalAttemptEvaluableSteps"], 2)
        self.assertEqual(task["finalAttemptCancelled"], 1)
        self.assertEqual(task["cancelledAttempts"], 1)
        only_cancelled = EVALUATOR.evaluate([cancelled])["taskKinds"]["generate_dmn_dsl"]
        self.assertIsNone(only_cancelled["firstAttemptSuccessRate"])
        self.assertIsNone(only_cancelled["finalAttemptSuccessRate"])

    def test_parent_child_steps_are_not_counted_as_retries(self):
        parent = orchestration(22, [step(1, 1, 1, "succeeded")])
        child = orchestration(23, [step(2, 1, 1, "succeeded", orchestration_id=23)])
        child["parentOrchestrationId"] = 22
        task = EVALUATOR.evaluate([child, parent])["taskKinds"]["generate_dmn_dsl"]
        self.assertEqual(task["logicalSteps"], 2)
        self.assertEqual(task["retryAttempts"], 0)
        self.assertEqual(task["firstAttemptSuccessRate"], 1.0)

    def test_fetch_history_pages_using_descending_before_id_cursor(self):
        requests = []
        pages = [
            [orchestration(22, []), orchestration(21, [])],
            [orchestration(19, [])],
        ]

        def fake_open(request, timeout):
            requests.append((request, timeout))
            return Response(pages.pop(0))

        with patch.object(EVALUATOR, "open_url", side_effect=fake_open):
            rows = EVALUATOR.fetch_history("https://api.example", "secret", 7, page_size=2)
        self.assertEqual([row["id"] for row in rows], [22, 21, 19])
        self.assertIn("limit=2", requests[0][0].full_url)
        self.assertIn("beforeId=21", requests[1][0].full_url)
        self.assertEqual(requests[0][0].get_header("Authorization"), "Bearer secret")

    def test_report_file_is_private_and_contains_only_aggregates(self):
        with tempfile.TemporaryDirectory() as directory:
            output = os.path.join(directory, "report.json")
            report = EVALUATOR.evaluate([orchestration(22, [step(1, 1, 1, "succeeded")])])
            EVALUATOR.write_report(report, output)
            self.assertEqual(os.stat(output).st_mode & 0o777, 0o600)
            with open(output, encoding="utf-8") as source:
                content = source.read()
            self.assertNotIn("artifactDigest", content)
            self.assertNotIn("secret", content)
            self.assertIn('"orchestrationCount": 1', content)


if __name__ == "__main__":
    unittest.main()
