#!/usr/bin/env python3
"""Evaluate blinded decision-explanation readability reviews from JSONL."""

import argparse
from collections import Counter, defaultdict
import json
import math
import sys

MAX_RECORDS = 100_000
MAX_LINE_BYTES = 65_536
MAX_I64 = 9_223_372_036_854_775_807
EXPECTED_FIELDS = {"explanationId", "explanationRevision", "reviewer", "label"}
LABELS = {"readable", "needs_revision", "inconclusive"}


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def validate_row(row, line_number):
    if not isinstance(row, dict):
        raise ValueError(f"line {line_number}: record must be an object")
    if set(row) != EXPECTED_FIELDS:
        missing = sorted(EXPECTED_FIELDS - set(row))
        extra = sorted(set(row) - EXPECTED_FIELDS)
        details = []
        if missing:
            details.append("missing fields: " + ", ".join(missing))
        if extra:
            details.append("unexpected fields: " + ", ".join(extra))
        raise ValueError(f"line {line_number}: invalid fields ({'; '.join(details)})")
    explanation_id = row["explanationId"]
    if type(explanation_id) is not int or not 0 < explanation_id <= MAX_I64:
        raise ValueError(f"line {line_number}: explanationId must be a positive integer")
    revision = row["explanationRevision"]
    if type(revision) is not int or not 0 < revision <= MAX_I64:
        raise ValueError(f"line {line_number}: explanationRevision must be a positive integer")
    if not isinstance(row["reviewer"], str) or not row["reviewer"] or len(row["reviewer"]) > 255:
        raise ValueError(f"line {line_number}: reviewer must be a non-empty string up to 255 characters")
    if not isinstance(row["label"], str) or row["label"] not in LABELS:
        raise ValueError(f"line {line_number}: unsupported review label")
    return row


def wilson_interval(successes, total, z=1.959963984540054):
    if total <= 0:
        return None
    rate = successes / total
    denominator = 1 + z * z / total
    center = (rate + z * z / (2 * total)) / denominator
    margin = z * math.sqrt(rate * (1 - rate) / total + z * z / (4 * total * total)) / denominator
    return {"lower": round(max(0.0, center - margin), 6), "upper": round(min(1.0, center + margin), 6)}


def build_report(rows, minimum_cases=100, target_rate=0.85):
    if type(minimum_cases) is not int or minimum_cases < 1:
        raise ValueError("minimum_cases must be a positive integer")
    if (not isinstance(target_rate, (int, float)) or isinstance(target_rate, bool)
            or not math.isfinite(target_rate) or not 0 <= target_rate <= 1):
        raise ValueError("target_rate must be a finite ratio from 0 to 1")

    by_explanation = defaultdict(Counter)
    latest_revision = {}
    seen_reviews = set()
    for index, row in enumerate(rows, 1):
        validate_row(row, index)
        key = (row["explanationId"], row["explanationRevision"], row["reviewer"])
        if key in seen_reviews:
            raise ValueError(f"duplicate reviewer/explanation pair at row {index}")
        seen_reviews.add(key)
        latest_revision[row["explanationId"]] = max(
            row["explanationRevision"], latest_revision.get(row["explanationId"], 0)
        )

    superseded_reviews = 0
    for row in rows:
        if row["explanationRevision"] != latest_revision[row["explanationId"]]:
            superseded_reviews += 1
            continue
        by_explanation[(row["explanationId"], row["explanationRevision"])][row["label"]] += 1

    case_counts = Counter()
    readable = 0
    determinate = 0
    for labels in by_explanation.values():
        positive = labels["readable"]
        negative = labels["needs_revision"]
        case_counts["reviewed"] += 1
        if positive == negative:
            case_counts["inconclusive"] += 1
            continue
        determinate += 1
        case_counts["determinate"] += 1
        if positive > negative:
            readable += 1
            case_counts["readable"] += 1
        else:
            case_counts["needs_revision"] += 1

    rate = readable / determinate if determinate else None
    eligible = determinate >= minimum_cases
    report = {
        "evaluationCount": len(rows),
        "uniqueExplanationCount": len({key[0] for key in by_explanation}),
        "uniqueExplanationRevisionCount": len({key[:2] for key in seen_reviews}),
        "countedReviewCount": len(rows) - superseded_reviews,
        "supersededReviewCount": superseded_reviews,
        "revisionPolicy": "latest_supplied_revision_per_explanation",
        "caseCounts": dict(case_counts),
        "readability": {
            "readableCases": readable,
            "determinateCases": determinate,
            "rate": round(rate, 6) if rate is not None else None,
            "wilson95": wilson_interval(readable, determinate),
            "targetRate": target_rate,
            "minimumCases": minimum_cases,
            "status": (
                "insufficient_samples" if not eligible
                else "target_met" if rate >= target_rate
                else "target_not_met"
            ),
        },
        "reviewersAndExplanationIdsOmitted": True,
    }
    return report


def load_records(path):
    rows = []
    with open(path, encoding="utf-8") as source:
        for line_number, line in enumerate(source, 1):
            if len(line.encode("utf-8")) > MAX_LINE_BYTES:
                raise ValueError(f"{path}:{line_number}: line exceeds {MAX_LINE_BYTES} bytes")
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            try:
                row = json.loads(line, object_pairs_hook=_unique_object)
            except (json.JSONDecodeError, ValueError) as error:
                raise ValueError(f"{path}:{line_number}: invalid JSON: {error}") from error
            rows.append(validate_row(row, line_number))
            if len(rows) > MAX_RECORDS:
                raise ValueError(f"{path}: exceeds {MAX_RECORDS} records")
    return rows


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", help="JSONL rows with explanationId, explanationRevision, reviewer, and label")
    parser.add_argument("--minimum-cases", type=int, default=100)
    parser.add_argument("--target-rate", type=float, default=0.85)
    args = parser.parse_args(argv)
    try:
        report = build_report(
            load_records(args.input),
            minimum_cases=args.minimum_cases,
            target_rate=args.target_rate,
        )
        print(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False))
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
