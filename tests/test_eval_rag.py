import importlib.util
import contextlib
import io
import json
import os
import tempfile
import unittest
from unittest.mock import patch


SCRIPT_PATH = os.path.join(os.path.dirname(__file__), "..", "script", "eval-rag.py")
SPEC = importlib.util.spec_from_file_location("eval_rag", SCRIPT_PATH)
EVAL_RAG = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EVAL_RAG)


class JsonResponse:
    def __init__(self, payload):
        self.body = json.dumps(payload).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, *_):
        return None

    def read(self, limit):
        return self.body[:limit]


def metrics_with_hits(keyword, fused, vector=0):
    metrics = EVAL_RAG.new_metrics()
    for stage, count in (("keyword", keyword), ("vector", vector), ("fused", fused)):
        metrics[stage]["hitsAt5"] = count
    return metrics


class RagEvaluationTests(unittest.TestCase):
    def test_load_cases_validates_positive_unique_ids_and_query_byte_limit(self):
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write("decision\t3,4\n")
            fixture.flush()
            self.assertEqual(EVAL_RAG.load_cases(fixture.name), [("decision", {3, 4})])

        invalid_rows = [
            ("query\t0\n", "positive integers"),
            ("query\t2,nope\n", "positive integers"),
            ("query\t2,2\n", "duplicate relevant IDs"),
            ("汉" * 667 + "\t1\n", "2000 UTF-8 bytes"),
        ]
        for row, message in invalid_rows:
            with self.subTest(message=message), tempfile.NamedTemporaryFile(
                mode="w", encoding="utf-8"
            ) as fixture:
                fixture.write(row)
                fixture.flush()
                with self.assertRaisesRegex(ValueError, message):
                    EVAL_RAG.load_cases(fixture.name)

    def test_load_categories_requires_one_bounded_category_per_case(self):
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write('{"category":"simple"}\n{"category":"complex"}\n')
            fixture.flush()
            self.assertEqual(EVAL_RAG.load_categories(fixture.name, 2), ["simple", "complex"])

        invalid_records = [
            ('{"category":" "}\n', "1-100 characters"),
            ('{"category":2}\n', "only a string category"),
            ('{"category":"simple","query":"secret"}\n', "only a string category"),
            ('{"category":"a","category":"b"}\n', "duplicate JSON key"),
            ('{"category":"' + "x" * 101 + '"}\n', "1-100 characters"),
            ('not-json\n', "JSON object"),
        ]
        for contents, message in invalid_records:
            with self.subTest(message=message), tempfile.NamedTemporaryFile(
                mode="w", encoding="utf-8"
            ) as fixture:
                fixture.write(contents)
                fixture.flush()
                with self.assertRaisesRegex(ValueError, message):
                    EVAL_RAG.load_categories(fixture.name, 1)

        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write('{"category":"only-one"}\n')
            fixture.flush()
            with self.assertRaisesRegex(ValueError, "expected exactly 2"):
                EVAL_RAG.load_categories(fixture.name, 2)

    def test_candidate_ids_validates_knowledge_and_pattern_response_shapes(self):
        knowledge = {
            "keyword_results": [{"id": 7}],
            "vector_results": [],
            "fused_results": [{"id": 7}],
        }
        self.assertEqual(EVAL_RAG.candidate_ids(knowledge, "fused", "knowledge"), [7])

        patterns = {
            "keywordCandidateIds": [7],
            "vectorCandidates": [[8, 0.9]],
            "rrfCandidateIds": [8, 7],
        }
        self.assertEqual(EVAL_RAG.candidate_ids(patterns, "vector", "patterns"), [8])
        self.assertEqual(EVAL_RAG.candidate_ids(patterns, "fused", "patterns"), [8, 7])

        with self.assertRaisesRegex(ValueError, "positive integer"):
            EVAL_RAG.candidate_ids({"keywordCandidateIds": [True]}, "keyword", "patterns")
        with self.assertRaisesRegex(ValueError, "JSON object"):
            EVAL_RAG.candidate_ids([], "fused", "knowledge")
        with self.assertRaisesRegex(ValueError, "must be an array"):
            EVAL_RAG.candidate_ids({}, "fused", "knowledge")

    def test_query_text_is_excluded_from_report_records_by_default(self):
        entry = EVAL_RAG.query_report_entry(1, "customer account 123", 1.234, {"fused": True})
        self.assertNotIn("query", entry)
        self.assertEqual(entry["caseIndex"], 1)
        included = EVAL_RAG.query_report_entry(
            1, "customer account 123", 1.234, {"fused": True}, include_query_text=True
        )
        self.assertEqual(included["query"], "customer account 123")

    def test_report_marks_latency_percentiles_with_insufficient_samples(self):
        metrics = {
            name: {
                "hitsAt5": 1,
                "recallAt5": 1.0,
                "relevantHitsAt5": 2,
                "relevantCount": 2,
                "mrr": 1.0,
                "ndcgAt5": 1.0,
            }
            for name in ("keyword", "vector", "fused")
        }
        report = EVAL_RAG.build_report("knowledge", metrics, [10.0], [], 30)
        self.assertEqual(report["latencySampleCount"], 1)
        self.assertEqual(report["latencySampleMinimum"], 30)
        self.assertFalse(report["latencySampleSufficient"])
        self.assertEqual(report["top5HitRate"]["fused"], 1.0)
        self.assertEqual(report["rankingMetrics"]["fused"]["mrr"], 1.0)
        uncertainty = report["metricUncertainty"]["fused"]
        self.assertEqual(uncertainty["top5HitRate"]["successes"], 1)
        self.assertEqual(uncertainty["top5HitRate"]["denominator"], 1)
        self.assertEqual(uncertainty["recallAt5Micro"]["denominator"], 2)
        self.assertEqual(uncertainty["recallAt5Micro"]["wilson95"], {
            "lower": 0.3424, "upper": 1.0
        })

    def test_stratified_report_uses_query_and_relevant_item_denominators(self):
        simple = EVAL_RAG.new_metrics()
        complex_case = EVAL_RAG.new_metrics()
        EVAL_RAG.add_metric_sample(
            simple, "fused", EVAL_RAG.ranking_metrics([1, 2], {1, 2}), True
        )
        EVAL_RAG.add_metric_sample(
            complex_case, "fused", EVAL_RAG.ranking_metrics([9], {1, 2}), False
        )
        report = EVAL_RAG.build_report(
            "knowledge",
            EVAL_RAG.new_metrics(),
            [10.0, 20.0],
            [],
            30,
            {"simple": (simple, 1), "complex": (complex_case, 1)},
        )

        self.assertEqual(set(report["stratifiedMetrics"]), {"complex", "simple"})
        simple_fused = report["stratifiedMetrics"]["simple"]["stages"]["fused"]
        self.assertEqual(simple_fused["top5HitRate"]["denominator"], 1)
        self.assertEqual(simple_fused["top5HitRate"]["value"], 1.0)
        self.assertEqual(simple_fused["recallAt5Micro"]["successes"], 2)
        self.assertEqual(simple_fused["recallAt5Micro"]["denominator"], 2)
        self.assertEqual(simple_fused["recallAt5Micro"]["value"], 1.0)
        complex_fused = report["stratifiedMetrics"]["complex"]["stages"]["fused"]
        self.assertEqual(complex_fused["recallAt5Micro"]["denominator"], 2)
        self.assertEqual(complex_fused["recallAt5Micro"]["successes"], 0)
        self.assertEqual(complex_fused["recallAt5Micro"]["wilson95"], {
            "lower": 0.0, "upper": 0.6576
        })

    def test_legacy_report_omits_stratified_metrics(self):
        report = EVAL_RAG.build_report(
            "knowledge", EVAL_RAG.new_metrics(), [1.0], [], 30
        )
        self.assertNotIn("stratifiedMetrics", report)

    def test_ranking_metrics_distinguish_order_and_multiple_relevant_results(self):
        first = EVAL_RAG.ranking_metrics([2, 1, 3], {1, 2})
        later = EVAL_RAG.ranking_metrics([8, 8, 1, 2], {1, 2})

        self.assertEqual(first["firstRelevantRank"], 1)
        self.assertEqual(first["recallAt5"], 1.0)
        self.assertEqual(first["mrr"], 1.0)
        self.assertEqual(later["firstRelevantRank"], 3)
        self.assertEqual(later["recallAt5"], 1.0)
        self.assertEqual(later["relevantHitsAt5"], 2)
        self.assertEqual(later["relevantCount"], 2)
        self.assertAlmostEqual(later["mrr"], 1 / 3)
        self.assertLess(later["ndcgAt5"], first["ndcgAt5"])

    def test_ranking_metrics_report_missing_relevant_results_as_zero(self):
        self.assertEqual(
            EVAL_RAG.ranking_metrics([7, 8], {1}),
            {
                "firstRelevantRank": None,
                "recallAt5": 0.0,
                "relevantHitsAt5": 0,
                "relevantCount": 1,
                "mrr": 0.0,
                "ndcgAt5": 0.0,
            },
        )

    def test_wilson_interval_handles_empty_perfect_small_and_invalid_samples(self):
        self.assertIsNone(EVAL_RAG.wilson_interval(0, 0))
        self.assertEqual(EVAL_RAG.wilson_interval(1, 1), {"lower": 0.2065, "upper": 1.0})
        self.assertEqual(EVAL_RAG.wilson_interval(0, 1), {"lower": 0.0, "upper": 0.7935})
        self.assertEqual(EVAL_RAG.wilson_interval(50, 100), {"lower": 0.4038, "upper": 0.5962})
        for successes, denominator in ((-1, 2), (3, 2), (1, -1), (True, 1)):
            with self.subTest(successes=successes, denominator=denominator):
                with self.assertRaises(ValueError):
                    EVAL_RAG.wilson_interval(successes, denominator)

    def test_quality_threshold_uses_paired_query_denominator_and_exact_15pp(self):
        for fused, expected in ((70, "target_met"), (69, "target_not_met")):
            report = EVAL_RAG.build_report(
                "knowledge", metrics_with_hits(55, fused), [10.0] * 100, [], 30,
                vector_statuses=["ready"] * 100,
            )
            quality = report["acceptance"]["quality"]
            self.assertEqual(quality["denominator"], 100)
            self.assertEqual(quality["fusedHitsMinusKeywordHits"], fused - 55)
            self.assertEqual(quality["status"], expected)
            self.assertFalse(report["acceptance"]["accepted"])
        small = EVAL_RAG.build_report("knowledge", metrics_with_hits(0, 99), [10.0] * 99, [], 30)
        self.assertEqual(small["acceptance"]["quality"]["status"], "insufficient_samples")
        self.assertEqual(small["acceptance"]["status"], "insufficient_samples")

    def test_adequate_debug_samples_never_claim_representative_load_acceptance(self):
        report = EVAL_RAG.build_report(
            "knowledge", metrics_with_hits(0, 100), [500.0] * 100, [], 30,
            vector_statuses=["ready"] * 100,
        )
        self.assertTrue(report["latencySampleSufficient"])
        self.assertEqual(report["acceptance"]["latency"]["status"], "diagnostic_target_met")
        self.assertEqual(report["latencyMeasurementScope"], "sequential_debug_endpoint_round_trip")
        self.assertFalse(report["representativeLoadVerified"])
        self.assertFalse(report["embeddingEvidence"]["providerAndModelVerified"])
        self.assertEqual(report["acceptance"]["status"], "insufficient_evidence")
        self.assertIn("representative_load_p95_unverified", report["acceptance"]["reasons"])
        slower = EVAL_RAG.build_report("knowledge", metrics_with_hits(0, 100), [500.01] * 100, [], 30)
        self.assertEqual(slower["acceptance"]["latency"]["status"], "diagnostic_target_not_met")

    def test_vector_evidence_counts_ready_no_matches_degraded_and_missing_separately(self):
        statuses = ["ready", "no_matches", "disabled", "pgvector_unavailable", "provider_error", "query_error", "not_run", "unknown"]
        report = EVAL_RAG.build_report("knowledge", metrics_with_hits(0, 8), [10.0] * 8, [], 1,
                                      minimum_quality_samples=1, vector_statuses=statuses)
        evidence = report["embeddingEvidence"]
        self.assertEqual(evidence["serverReportedVectorExecutionCount"], 2)
        self.assertEqual(evidence["unknownStatusCount"], 1)
        self.assertEqual(evidence["degradedOrNotRunCount"], 5)
        self.assertIn("vector_stage_degraded_or_not_run", report["acceptance"]["reasons"])
        self.assertIn("vector_execution_status_missing", report["acceptance"]["reasons"])
        self.assertEqual(EVAL_RAG.observed_vector_status({"vector_candidates": [[1, 0.5]]}, "patterns"), "unknown")
        for invalid in ({"vector_status": True}, {"vector_status": "made-up"}, []):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                EVAL_RAG.observed_vector_status(invalid, "knowledge")

    def test_report_rejects_nonfinite_latencies_metrics_invalid_samples_and_status_counts(self):
        for invalid in (float("nan"), float("inf"), -1.0, True, 10 ** 1000):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                EVAL_RAG.build_report("knowledge", metrics_with_hits(0, 1), [invalid], [], 30)
        for invalid in (True, 0, 100001):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                EVAL_RAG.build_report("knowledge", metrics_with_hits(0, 1), [1.0], [], invalid)
        for invalid in ([], ["unknown", "ready"], [True]):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                EVAL_RAG.build_report("knowledge", metrics_with_hits(0, 1), [1.0], [], 30, vector_statuses=invalid)
        metrics = metrics_with_hits(0, 1)
        metrics["fused"]["mrr"] = float("nan")
        with self.assertRaisesRegex(ValueError, "finite"):
            EVAL_RAG.build_report("knowledge", metrics, [1.0], [], 30)
        with self.assertRaises(ValueError):
            EVAL_RAG.build_report("knowledge", metrics_with_hits(0, 0), [], [], 30)

    def test_labeled_set_rejects_duplicate_queries_and_invalid_or_oversized_ids(self):
        for content, message in (
            ("same query\t1\n same query \t2\n", "duplicate labeled query"),
            ("query\t9223372036854775808\n", "signed 64-bit"),
            ("query\tTrue\n", "positive integers"),
            ("query\t1.5\n", "positive integers"),
            ("query\t+1\n", "positive integers"),
            ("query\t01\n", "positive integers"),
            ("query\x00\t1\n", "control characters"),
        ):
            with self.subTest(content=content), tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
                fixture.write(content)
                fixture.flush()
                with self.assertRaisesRegex(ValueError, message):
                    EVAL_RAG.load_cases(fixture.name)

    def test_local_input_size_limits_include_comment_lines(self):
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write("#" + "汉" * 100 + "\nquery\t1\n")
            fixture.flush()
            with patch.object(EVAL_RAG, "MAX_LINE_BYTES", 100), self.assertRaisesRegex(ValueError, "exceeds 100 bytes"):
                EVAL_RAG.load_cases(fixture.name)
            with patch.object(EVAL_RAG, "MAX_FILE_BYTES", 100), self.assertRaisesRegex(ValueError, "input exceeds"):
                EVAL_RAG.load_cases(fixture.name)
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write("first\t1\nsecond\t2\n")
            fixture.flush()
            with patch.object(EVAL_RAG, "MAX_RECORDS", 1), self.assertRaisesRegex(ValueError, "exceeds 1 cases"):
                EVAL_RAG.load_cases(fixture.name)

    def test_validate_only_needs_no_credentials_and_never_opens_network(self):
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write("private business query\t2,1\n")
            fixture.flush()
            output = io.StringIO()
            with patch.dict(os.environ, {}, clear=True), contextlib.redirect_stdout(output), patch.object(EVAL_RAG.urllib.request, "urlopen") as request:
                self.assertEqual(EVAL_RAG.main([fixture.name, "--validate-only"]), 0)
            request.assert_not_called()
            report = json.loads(output.getvalue())
            self.assertEqual(report["networkRequests"], 0)
            self.assertEqual(report["queryCount"], 1)
            self.assertFalse(report["accepted"])
            self.assertNotIn("private business query", output.getvalue())
            self.assertEqual(len(report["datasetDigest"]), 64)

    def test_entire_label_set_and_categories_validate_before_any_request(self):
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write("valid\t1\ninvalid\tnope\n")
            fixture.flush()
            with patch.object(EVAL_RAG.urllib.request, "urlopen") as request, self.assertRaises(ValueError):
                EVAL_RAG.main([fixture.name, "--token", "unused", "--project-id", "1"])
            request.assert_not_called()
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture, tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as categories:
            fixture.write("query\t1\n")
            fixture.flush()
            categories.write('{"category":NaN}\n')
            categories.flush()
            with patch.object(EVAL_RAG.urllib.request, "urlopen") as request, self.assertRaises(ValueError):
                EVAL_RAG.main([fixture.name, "--token", "unused", "--project-id", "1", "--categories", categories.name])
            request.assert_not_called()

    def test_dataset_fingerprint_is_stable_for_id_order_and_changes_with_labels(self):
        a = EVAL_RAG.dataset_digest([("query", {1, 2})], ["business"])
        self.assertEqual(a, EVAL_RAG.dataset_digest([("query", {2, 1})], ["business"]))
        self.assertNotEqual(a, EVAL_RAG.dataset_digest([("query", {1, 3})], ["business"]))
        self.assertNotEqual(a, EVAL_RAG.dataset_digest([("query", {1, 2})], ["other"]))

    def test_mock_live_report_records_one_stratified_query_and_missing_provider_evidence(self):
        payload = {"keyword_results": [], "vector_results": [{"id": 1, "similarity": 0.8}],
                   "fused_results": [{"id": 1}], "vector_status": "ready"}
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture, tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as categories:
            fixture.write("private query\t1\n")
            fixture.flush()
            categories.write('{"category":"business"}\n')
            categories.flush()
            output = io.StringIO()
            with contextlib.redirect_stdout(output), patch.object(EVAL_RAG.urllib.request, "urlopen", return_value=JsonResponse(payload)) as request:
                self.assertEqual(EVAL_RAG.main([fixture.name, "--token", "fixture-token", "--project-id", "1", "--categories", categories.name]), 0)
            self.assertEqual(request.call_count, 1)
            report = json.loads(output.getvalue())
            group = report["stratifiedMetrics"]["business"]
            self.assertEqual(group["queryCount"], 1)
            self.assertEqual(group["stages"]["fused"]["top5HitRate"]["denominator"], 1)
            self.assertEqual(group["stages"]["fused"]["top5HitRate"]["value"], 1.0)
            self.assertEqual(report["queries"][0]["vectorStatus"], "ready")
            self.assertFalse(report["acceptance"]["accepted"])
            self.assertNotIn("private query", output.getvalue())
            self.assertNotIn("fixture-token", output.getvalue())

    def test_api_scores_duplicate_candidates_and_nonfinite_json_are_rejected(self):
        for payload in (
            {"vectorCandidates": [[1, float("nan")]]},
            {"vectorCandidates": [[1, True]]},
            {"vectorCandidates": [[1, 10 ** 1000]]},
            {"vectorCandidates": [[1, 0.5, "extra"]]},
        ):
            with self.subTest(payload=payload), self.assertRaises(ValueError):
                EVAL_RAG.candidate_ids(payload, "vector", "patterns")
        with self.assertRaisesRegex(ValueError, "duplicate candidate"):
            EVAL_RAG.candidate_ids({"keywordCandidateIds": [1, 1]}, "keyword", "patterns")
        with self.assertRaisesRegex(ValueError, "finite"):
            EVAL_RAG.candidate_ids({"vector_results": [{"id": 1, "similarity": float("inf")}]}, "vector", "knowledge")
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8") as fixture:
            fixture.write("query\t1\n")
            fixture.flush()
            with patch.object(EVAL_RAG.urllib.request, "urlopen", return_value=JsonResponse({"score": float("nan")})), self.assertRaisesRegex(ValueError, "non-finite JSON"):
                EVAL_RAG.main([fixture.name, "--token", "fixture-token", "--project-id", "1"])


if __name__ == "__main__":
    unittest.main()
