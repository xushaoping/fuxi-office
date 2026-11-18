#!/usr/bin/env python3
"""Evaluate labeled knowledge retrieval queries against the protected debug API."""

import argparse
from collections import Counter
import hashlib
import json
import math
import os
import re
import statistics
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


MAX_RECORDS = 100_000
MAX_LINE_BYTES = 65_536
MAX_FILE_BYTES = 32 * 1024 * 1024
MAX_RESPONSE_BYTES = 8 * 1024 * 1024
MAX_I64 = 9_223_372_036_854_775_807
MAX_RELEVANT_IDS = 1000
VECTOR_STATUSES = {
    "ready", "no_matches", "disabled", "pgvector_unavailable", "provider_error",
    "query_error", "not_run",
}
VECTOR_EXECUTED_STATUSES = {"ready", "no_matches"}


def is_finite_number(value):
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        return False
    try:
        return math.isfinite(value)
    except OverflowError:
        return False


def finite_nonnegative(value, context):
    if not is_finite_number(value) or value < 0:
        raise ValueError(f"{context} must be a finite non-negative number")
    return value


def unique_object(pairs):
    record = {}
    for key, value in pairs:
        if key in record:
            raise ValueError("duplicate JSON key")
        record[key] = value
    return record


def reject_constant(_value):
    raise ValueError("non-finite JSON number")


def bounded_lines(path):
    consumed = 0
    with open(path, "rb") as source:
        line_number = 0
        while True:
            raw = source.readline(MAX_LINE_BYTES + 1)
            if not raw:
                break
            consumed += len(raw)
            line_number += 1
            if len(raw) > MAX_LINE_BYTES:
                raise ValueError(f"line {line_number}: exceeds {MAX_LINE_BYTES} bytes")
            if consumed > MAX_FILE_BYTES:
                raise ValueError(f"input exceeds {MAX_FILE_BYTES} bytes")
            try:
                yield line_number, raw.decode("utf-8").rstrip("\r\n")
            except UnicodeDecodeError as error:
                raise ValueError(f"line {line_number}: invalid UTF-8") from error


def percentile_95(values):
    ordered = sorted(values)
    if not ordered:
        return 0
    index = max(0, (95 * len(ordered) + 99) // 100 - 1)
    return ordered[index]


def wilson_interval(successes, denominator, z=1.959963984540054):
    if (type(successes) is not int or type(denominator) is not int
            or denominator < 0 or successes < 0 or successes > denominator):
        raise ValueError("Wilson interval requires 0 <= integer successes <= denominator")
    if denominator == 0:
        return None
    proportion = successes / denominator
    z_squared = z * z
    adjustment = 1 + z_squared / denominator
    center = (proportion + z_squared / (2 * denominator)) / adjustment
    margin = (z / adjustment) * math.sqrt(
        proportion * (1 - proportion) / denominator
        + z_squared / (4 * denominator * denominator)
    )
    return {"lower": round(max(0.0, center - margin), 4),
            "upper": round(min(1.0, center + margin), 4)}


def hit_at_5(results, relevant_ids):
    return any(item in relevant_ids for item in results[:5])


def ranking_metrics(results, relevant_ids, k=5):
    seen = set()
    relevant_ranks = []
    for index, item in enumerate(results, 1):
        if item in seen:
            continue
        seen.add(item)
        if item in relevant_ids:
            relevant_ranks.append(index)
    hits = sum(rank <= k for rank in relevant_ranks)
    dcg = sum(1 / math.log2(rank + 1) for rank in relevant_ranks if rank <= k)
    ideal_hits = min(len(relevant_ids), k)
    ideal_dcg = sum(1 / math.log2(rank + 1) for rank in range(1, ideal_hits + 1))
    first_rank = relevant_ranks[0] if relevant_ranks else None
    return {
        "firstRelevantRank": first_rank,
        "recallAt5": hits / len(relevant_ids),
        "relevantHitsAt5": hits,
        "relevantCount": len(relevant_ids),
        "mrr": 1 / first_rank if first_rank is not None else 0.0,
        "ndcgAt5": dcg / ideal_dcg if ideal_dcg else 0.0,
    }


def positive_id(value, context):
    if type(value) is not int or not 0 < value <= MAX_I64:
        raise ValueError(f"{context} must be a positive integer ID")
    return value


def candidate_ids(payload, name, kind):
    if not isinstance(payload, dict):
        raise ValueError("retrieval API response must be a JSON object")
    if kind == "knowledge":
        field = f"{name}_results"
        values = payload.get(field)
        if not isinstance(values, list):
            raise ValueError(f"retrieval API field {field} must be an array")
        ids = []
        for index, item in enumerate(values):
            if not isinstance(item, dict):
                raise ValueError(f"{field}[{index}] must be an object")
            ids.append(positive_id(item.get("id"), f"{field}[{index}].id"))
            if item.get("similarity") is not None:
                similarity = item["similarity"]
                if not is_finite_number(similarity):
                    raise ValueError(f"{field}[{index}].similarity must be finite")
        return unique_candidate_ids(ids)
    if name == "vector":
        values = payload.get("vectorCandidates")
        if not isinstance(values, list):
            raise ValueError("retrieval API field vectorCandidates must be an array")
        ids = []
        for index, item in enumerate(values):
            if not isinstance(item, list) or len(item) != 2:
                raise ValueError(f"vectorCandidates[{index}] must be an ID/score pair")
            ids.append(positive_id(item[0], f"vectorCandidates[{index}][0]"))
            if not is_finite_number(item[1]):
                raise ValueError(f"vectorCandidates[{index}][1] must be a finite score")
        return unique_candidate_ids(ids)
    field = "rrfCandidateIds" if name == "fused" else "keywordCandidateIds"
    values = payload.get(field)
    if not isinstance(values, list):
        raise ValueError(f"retrieval API field {field} must be an array")
    return unique_candidate_ids([positive_id(item, f"{field}[{index}]") for index, item in enumerate(values)])


def unique_candidate_ids(ids):
    if len(ids) > MAX_RELEVANT_IDS:
        raise ValueError("retrieval stage exceeds candidate limit")
    if len(ids) != len(set(ids)):
        raise ValueError("retrieval stage contains duplicate candidate IDs")
    return ids


def observed_vector_status(payload, kind):
    # Nonempty vectors alone do not prove a provider call or absence of fallback.
    if not isinstance(payload, dict):
        raise ValueError("retrieval API response must be a JSON object")
    status = payload.get("vector_status" if kind == "knowledge" else "vectorStatus")
    if status is None or status == "":
        return "unknown"
    if not isinstance(status, str) or status not in VECTOR_STATUSES:
        raise ValueError("retrieval API returned an unsupported vector status")
    return status


def query_report_entry(index, query, latency_ms, hits, include_query_text=False):
    item = {"caseIndex": index, "latencyMs": round(latency_ms, 2), "hitsAt5": hits}
    if include_query_text:
        item["query"] = query
    return item


def new_metrics():
    return {
        name: {
            "hitsAt5": 0,
            "recallAt5": 0.0,
            "relevantHitsAt5": 0,
            "relevantCount": 0,
            "mrr": 0.0,
            "ndcgAt5": 0.0,
        }
        for name in ("keyword", "vector", "fused")
    }


def add_metric_sample(metrics, name, scores, found):
    metrics[name]["hitsAt5"] += int(found)
    for field in ("recallAt5", "mrr", "ndcgAt5"):
        metrics[name][field] += scores[field]
    metrics[name]["relevantHitsAt5"] += scores["relevantHitsAt5"]
    metrics[name]["relevantCount"] += scores["relevantCount"]


def stratified_metric_report(metrics, query_count):
    stages = {}
    for name, values in metrics.items():
        relevant_count = values["relevantCount"]
        relevant_hits = values["relevantHitsAt5"]
        stages[name] = {
            "top5HitRate": {
                "successes": values["hitsAt5"],
                "denominator": query_count,
                "value": round(values["hitsAt5"] / query_count, 4),
                "wilson95": wilson_interval(values["hitsAt5"], query_count),
            },
            "recallAt5Micro": {
                "successes": relevant_hits,
                "denominator": relevant_count,
                "value": round(relevant_hits / relevant_count, 4) if relevant_count else None,
                "wilson95": wilson_interval(relevant_hits, relevant_count),
            },
        }
    return {"queryCount": query_count, "stages": stages}


def build_report(kind, metrics, durations_ms, per_query, minimum_latency_samples, strata=None,
                 minimum_quality_samples=100, vector_statuses=None):
    total = len(durations_ms)
    if not total or total > MAX_RECORDS:
        raise ValueError("report requires 1-100000 query samples")
    for name, value in (("minimum_latency_samples", minimum_latency_samples),
                        ("minimum_quality_samples", minimum_quality_samples)):
        if type(value) is not int or not 1 <= value <= MAX_RECORDS:
            raise ValueError(f"{name} must be an integer from 1 to {MAX_RECORDS}")
    for duration in durations_ms:
        finite_nonnegative(duration, "latency")
    if set(metrics) != {"keyword", "vector", "fused"}:
        raise ValueError("report requires keyword, vector and fused metrics")
    for values in metrics.values():
        wilson_interval(values["hitsAt5"], total)
        wilson_interval(values["relevantHitsAt5"], values["relevantCount"])
        for field in ("recallAt5", "mrr", "ndcgAt5"):
            finite_nonnegative(values[field], field)
            if values[field] > total:
                raise ValueError(f"{field} sum exceeds query count")
    if vector_statuses is None:
        vector_statuses = ["unknown"] * total
    if (len(vector_statuses) != total
            or any(not isinstance(status, str) or status not in VECTOR_STATUSES | {"unknown"} for status in vector_statuses)):
        raise ValueError("vector statuses must contain one supported value per query")
    hit_rates = {name: values["hitsAt5"] / total for name, values in metrics.items()}
    ranking = {
        name: {
            field: round(values[field] / total, 4)
            for field in ("recallAt5", "mrr", "ndcgAt5")
        }
        for name, values in metrics.items()
    }
    metric_uncertainty = {}
    for name, values in metrics.items():
        relevant_count = values["relevantCount"]
        relevant_hits = values["relevantHitsAt5"]
        metric_uncertainty[name] = {
            "top5HitRate": {
                "successes": values["hitsAt5"],
                "denominator": total,
                "wilson95": wilson_interval(values["hitsAt5"], total),
            },
            "recallAt5Micro": {
                "successes": relevant_hits,
                "denominator": relevant_count,
                "wilson95": wilson_interval(relevant_hits, relevant_count),
            },
        }
    report = {
        "kind": kind,
        "queryCount": total,
        "top5HitRate": {name: round(rate, 4) for name, rate in hit_rates.items()},
        "rankingMetrics": ranking,
        "metricUncertainty": metric_uncertainty,
        "fusedUpliftVsKeywordPercentagePoints": round(
            (hit_rates["fused"] - hit_rates["keyword"]) * 100, 2
        ),
        "latencySampleCount": total,
        "latencySampleMinimum": minimum_latency_samples,
        "latencySampleSufficient": total >= minimum_latency_samples,
        "p95LatencyMs": round(percentile_95(durations_ms), 2),
        "meanLatencyMs": round(statistics.fmean(durations_ms), 2),
        "queries": per_query,
    }
    report["acceptance"] = acceptance_report(
        metrics, durations_ms, minimum_quality_samples, minimum_latency_samples, vector_statuses
    )
    report["embeddingEvidence"] = {
        "source": "retrieval_api_vector_status",
        "statusCounts": dict(sorted(Counter(vector_statuses).items())),
        "serverReportedVectorExecutionCount": sum(status in VECTOR_EXECUTED_STATUSES for status in vector_statuses),
        "unknownStatusCount": vector_statuses.count("unknown"),
        "degradedOrNotRunCount": sum(status not in VECTOR_EXECUTED_STATUSES | {"unknown"} for status in vector_statuses),
        "providerAndModelVerified": False,
    }
    report["latencyMeasurementScope"] = "sequential_debug_endpoint_round_trip"
    report["representativeLoadVerified"] = False
    if strata is not None:
        report["stratifiedMetrics"] = {
            category: stratified_metric_report(category_metrics, category_count)
            for category, (category_metrics, category_count) in sorted(strata.items())
        }
    return report


def acceptance_report(metrics, durations_ms, minimum_quality_samples, minimum_latency_samples, vector_statuses):
    total = len(durations_ms)
    difference = metrics["fused"]["hitsAt5"] - metrics["keyword"]["hitsAt5"]
    sufficient_quality = total >= minimum_quality_samples
    uplift_met = difference * 100 >= 15 * total
    sufficient_latency = total >= minimum_latency_samples
    latency_met = percentile_95(durations_ms) <= 500
    reasons = []
    if not sufficient_quality:
        reasons.append("insufficient_labeled_queries")
    if not sufficient_latency:
        reasons.append("insufficient_latency_samples")
    if sufficient_quality and not uplift_met:
        reasons.append("top5_uplift_below_15_percentage_points")
    if sufficient_latency and not latency_met:
        reasons.append("observed_debug_p95_exceeds_500ms")
    if any(status == "unknown" for status in vector_statuses):
        reasons.append("vector_execution_status_missing")
    if any(status not in VECTOR_EXECUTED_STATUSES | {"unknown"} for status in vector_statuses):
        reasons.append("vector_stage_degraded_or_not_run")
    reasons.extend(["embedding_provider_and_model_unverified", "representative_load_p95_unverified"])
    if not sufficient_quality or not sufficient_latency:
        overall = "insufficient_samples"
    elif not uplift_met:
        overall = "target_not_met"
    else:
        overall = "insufficient_evidence"
    return {
        "accepted": False,
        "status": overall,
        "reasons": reasons,
        "quality": {
            "metric": "paired_query_top5_hit_rate_uplift",
            "denominator": total,
            "minimumQueryCount": minimum_quality_samples,
            "fusedHitsMinusKeywordHits": difference,
            "targetUpliftPercentagePoints": 15,
            "status": "insufficient_samples" if not sufficient_quality else "target_met" if uplift_met else "target_not_met",
        },
        "latency": {
            "targetP95Ms": 500,
            "sampleCount": total,
            "minimumSampleCount": minimum_latency_samples,
            "status": "insufficient_samples" if not sufficient_latency else "diagnostic_target_met" if latency_met else "diagnostic_target_not_met",
            "eligibleForProductionAcceptance": False,
        },
    }


def load_cases(path, max_query_chars=2000):
    cases = []
    seen_queries = set()
    for line_number, line in bounded_lines(path):
        if not line or line.startswith("#"):
            continue
        fields = line.split("\t")
        query = fields[0].strip() if fields else ""
        if len(fields) != 2 or not query or not fields[1].strip():
            raise ValueError(f"line {line_number}: expected query<TAB>relevant_chunk_ids")
        if len(query) > max_query_chars or len(query.encode("utf-8")) > 2000:
            raise ValueError(f"line {line_number}: query exceeds 2000 UTF-8 bytes or {max_query_chars} characters")
        if any(ord(character) < 32 or ord(character) == 127 for character in query):
            raise ValueError(f"line {line_number}: query contains control characters")
        if query in seen_queries:
            raise ValueError(f"line {line_number}: duplicate labeled query")
        id_fields = [value.strip() for value in fields[1].split(",")]
        if len(id_fields) > MAX_RELEVANT_IDS:
            raise ValueError(f"line {line_number}: too many relevant IDs")
        if any(not re.fullmatch(r"[1-9][0-9]{0,18}", value) for value in id_fields):
            raise ValueError(f"line {line_number}: relevant IDs must be positive integers")
        parsed_ids = [int(value) for value in id_fields]
        if any(value > MAX_I64 for value in parsed_ids):
            raise ValueError(f"line {line_number}: relevant IDs must be positive integers within signed 64-bit range")
        if len(parsed_ids) != len(set(parsed_ids)):
            raise ValueError(f"line {line_number}: duplicate relevant IDs")
        cases.append((query, set(parsed_ids)))
        seen_queries.add(query)
        if len(cases) > MAX_RECORDS:
            raise ValueError(f"input exceeds {MAX_RECORDS} cases")
    if not cases:
        raise ValueError("the labeled retrieval set has no cases")
    return cases


def load_categories(path, expected_count):
    categories = []
    for line_number, line in bounded_lines(path):
        try:
            record = json.loads(line, object_pairs_hook=unique_object, parse_constant=reject_constant)
        except (json.JSONDecodeError, RecursionError) as error:
            raise ValueError(f"line {line_number}: expected a JSON object with category") from error
        except ValueError as error:
            raise ValueError(f"line {line_number}: {error}") from error
        if (not isinstance(record, dict) or set(record) != {"category"}
                or not isinstance(record["category"], str)):
            raise ValueError(f"line {line_number}: expected only a string category field")
        category = record["category"].strip()
        if (not category or len(category) > 100 or len(category.encode("utf-8")) > 200
                or any(ord(character) < 32 or ord(character) == 127 for character in category)):
            raise ValueError(f"line {line_number}: category must be 1-100 characters and at most 200 UTF-8 bytes without controls")
        categories.append(category)
        if len(categories) > MAX_RECORDS:
            raise ValueError(f"input exceeds {MAX_RECORDS} categories")
    if len(categories) != expected_count:
        raise ValueError(f"expected exactly {expected_count} category records, found {len(categories)}")
    return categories


def dataset_digest(cases, categories=None):
    # IDs remain in the private dataset, while only its fingerprint enters reports.
    canonical = [{"query": query, "relevantIds": sorted(ids),
                  "category": categories[index] if categories is not None else None}
                 for index, (query, ids) in enumerate(cases)]
    raw = json.dumps(canonical, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(raw.encode("utf-8")).hexdigest()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("golden_set", help="UTF-8 TSV: query<TAB>comma-separated relevant chunk IDs")
    parser.add_argument("--api-base", default=os.environ.get("API_BASE", "http://127.0.0.1:3000"))
    parser.add_argument("--token", default=os.environ.get("API_TOKEN"))
    parser.add_argument("--project-id", default=os.environ.get("PROJECT_ID"))
    parser.add_argument("--output", help="write JSON report to this path instead of stdout")
    parser.add_argument("--kind", choices=("knowledge", "patterns"), default="knowledge")
    parser.add_argument("--include-query-text", action="store_true",
                        help="include query text in the report; disabled by default")
    parser.add_argument("--categories", help="optional JSONL with one {\"category\": \"...\"} record per golden-set row")
    parser.add_argument("--minimum-latency-samples", type=int, default=30,
                        help="minimum sample count for the diagnostic latency comparison, not production acceptance")
    parser.add_argument("--minimum-quality-samples", type=int, default=100)
    parser.add_argument("--validate-only", action="store_true",
                        help="validate all local labels and categories without a token or network request")
    args = parser.parse_args(argv)
    if not 1 <= args.minimum_latency_samples <= MAX_RECORDS:
        parser.error("--minimum-latency-samples must be 1-100000")
    if not 1 <= args.minimum_quality_samples <= MAX_RECORDS:
        parser.error("--minimum-quality-samples must be 1-100000")
    # Validate the entire local corpus before credentials are needed or any request starts.
    cases = load_cases(args.golden_set, max_query_chars=500 if args.kind == "patterns" else 2000)
    categories = load_categories(args.categories, len(cases)) if args.categories else None
    digest = dataset_digest(cases, categories)
    if args.validate_only:
        report = {"mode": "validate_only", "kind": args.kind, "queryCount": len(cases),
                  "datasetDigest": digest, "categoryCount": len(set(categories)) if categories else 0,
                  "qualitySampleMinimum": args.minimum_quality_samples,
                  "qualitySampleSufficient": len(cases) >= args.minimum_quality_samples,
                  "networkRequests": 0, "accepted": False}
        render_report(report, args.output)
        return 0
    if not args.token or not args.token.strip():
        parser.error("--token or API_TOKEN is required for live diagnostic evaluation")
    if args.kind == "knowledge" and not args.project_id:
        parser.error("--project-id or PROJECT_ID is required for knowledge evaluation")
    if args.project_id is not None:
        if not re.fullmatch(r"[1-9][0-9]{0,18}", str(args.project_id)) or int(args.project_id) > MAX_I64:
            parser.error("--project-id must be a positive signed 64-bit integer")
    headers = {"Authorization": f"Bearer {args.token}", "Accept": "application/json"}
    metrics = new_metrics()
    strata = {} if categories is not None else None
    if strata is not None:
        for category in categories:
            if category not in strata:
                strata[category] = (new_metrics(), 0)
    durations_ms = []
    per_query = []
    vector_statuses = []
    if args.kind == "knowledge":
        endpoint = args.api_base.rstrip("/") + f"/api/projects/{args.project_id}/knowledge-search-debug"
    else:
        endpoint = args.api_base.rstrip("/") + "/api/model-library/search-debug"

    for case_index, (query, relevant_ids) in enumerate(cases):
        params = {"q": query}
        if args.project_id:
            params["projectId"] = args.project_id
        url = endpoint + "?" + urllib.parse.urlencode(params)
        request = urllib.request.Request(url, headers=headers)
        started = time.perf_counter()
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read(MAX_RESPONSE_BYTES + 1)
                if len(raw) > MAX_RESPONSE_BYTES:
                    raise ValueError("retrieval API response exceeds size limit")
                payload = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=reject_constant)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as error:
            raise RuntimeError(f"retrieval request failed for case {len(durations_ms) + 1}: {error}") from error
        elapsed_ms = (time.perf_counter() - started) * 1000
        finite_nonnegative(elapsed_ms, "observed latency")
        durations_ms.append(elapsed_ms)
        vector_statuses.append(observed_vector_status(payload, args.kind))
        case_hits = {}
        case_ranks = {}
        for name in metrics:
            ids = candidate_ids(payload, name, args.kind)
            scores = ranking_metrics(ids, relevant_ids)
            found = scores["firstRelevantRank"] is not None and scores["firstRelevantRank"] <= 5
            add_metric_sample(metrics, name, scores, found)
            if strata is not None:
                category = categories[case_index]
                category_metrics, category_count = strata[category]
                add_metric_sample(category_metrics, name, scores, found)
            case_hits[name] = found
            case_ranks[name] = scores
        if strata is not None:
            strata[category] = (category_metrics, category_count + 1)
        entry = query_report_entry(
            len(per_query) + 1, query, elapsed_ms, case_hits, args.include_query_text
        )
        entry["rankingMetrics"] = case_ranks
        entry["vectorStatus"] = vector_statuses[-1]
        per_query.append(entry)

    report = build_report(
        args.kind, metrics, durations_ms, per_query, args.minimum_latency_samples, strata,
        minimum_quality_samples=args.minimum_quality_samples, vector_statuses=vector_statuses
    )
    report["datasetDigest"] = digest
    render_report(report, args.output)
    return 0


def render_report(report, output):
    rendered = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    if output:
        with open(output, "w", encoding="utf-8") as target:
            target.write(rendered)
    else:
        sys.stdout.write(rendered)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
