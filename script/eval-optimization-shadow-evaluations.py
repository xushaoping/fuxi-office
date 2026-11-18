#!/usr/bin/env python3
"""Summarize exported optimization shadow evaluations without claiming adoption."""

import argparse
from collections import Counter
import json
import math
import os
from pathlib import Path
import sys
import tempfile

MAX_RECORDS = 100_000
MAX_LINE_BYTES = 65_536
MAX_I64 = 9_223_372_036_854_775_807
EXPECTED_FIELDS = {
    "id", "simulationId", "projectId", "proposalId", "sourceVersionId",
    "candidateVersionId", "outcome", "findingCode", "sampleSource",
    "testSetSize", "baselineAccuracy", "simulatedAccuracy", "createdAt",
}
OUTCOMES = {
    "blocked_project_risk", "blocked_finding_code", "blocked_unlabeled_samples",
    "blocked_insufficient_samples", "blocked_simulation", "blocked_change_risk",
    "eligible_shadow_candidate",
}
FINDINGS = {"HIGH_FAILURE_RATE", "HIGH_MISMATCH_RATE"}
SAMPLE_SOURCES = {"approved_business_tests", "successful_history_runs"}


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
    fields = set(row)
    if fields != EXPECTED_FIELDS:
        extra = sorted(fields - EXPECTED_FIELDS)
        missing = sorted(EXPECTED_FIELDS - fields)
        detail = []
        if extra:
            detail.append("unexpected fields: " + ", ".join(extra))
        if missing:
            detail.append("missing fields: " + ", ".join(missing))
        raise ValueError(f"line {line_number}: invalid fields ({'; '.join(detail)})")

    for field in ("id", "simulationId", "projectId", "proposalId", "sourceVersionId", "candidateVersionId"):
        value = row[field]
        if type(value) is not int or value <= 0 or value > MAX_I64:
            raise ValueError(f"line {line_number}: {field} must be a positive integer")
    if row["testSetSize"] is None or type(row["testSetSize"]) is not int or not 0 <= row["testSetSize"] <= 1000:
        raise ValueError(f"line {line_number}: testSetSize must be an integer from 0 to 1000")
    if not isinstance(row["outcome"], str) or row["outcome"] not in OUTCOMES:
        raise ValueError(f"line {line_number}: unsupported outcome")
    if not isinstance(row["findingCode"], str) or row["findingCode"] not in FINDINGS:
        raise ValueError(f"line {line_number}: unsupported findingCode")
    if not isinstance(row["sampleSource"], str) or row["sampleSource"] not in SAMPLE_SOURCES:
        raise ValueError(f"line {line_number}: unsupported sampleSource")
    if not isinstance(row["createdAt"], str) or not row["createdAt"] or len(row["createdAt"]) > 64:
        raise ValueError(f"line {line_number}: invalid createdAt")
    for field in ("baselineAccuracy", "simulatedAccuracy"):
        value = row[field]
        if (not isinstance(value, (int, float)) or isinstance(value, bool)
                or not math.isfinite(value) or not 0 <= value <= 100):
            raise ValueError(f"line {line_number}: {field} must be a finite percentage")
    return row


def _group_summary(rows):
    count = len(rows)
    differences = [row["simulatedAccuracy"] - row["baselineAccuracy"] for row in rows]
    improved = sum(delta > 0 for delta in differences)
    unchanged = sum(delta == 0 for delta in differences)
    regressed = count - improved - unchanged
    return {
        "count": count,
        "accuracyChangePp": {
            "mean": round(sum(differences) / count, 4) if count else None,
            "improved": improved,
            "unchanged": unchanged,
            "regressed": regressed,
            "denominator": count,
        },
    }


def _distribution(rows, field):
    buckets = {}
    for key, count in sorted(Counter(row[field] for row in rows).items()):
        buckets[key] = count
    return buckets


def build_report(rows):
    dimensions = ("outcome", "findingCode", "sampleSource")
    report = {
        "evaluationCount": len(rows),
        "shadowQualificationIsNotProductionAcceptance": True,
        "distributions": {field: _distribution(rows, field) for field in dimensions},
        "simulationAccuracyChange": _group_summary(rows),
        "byOutcome": {},
        "byFindingCode": {},
        "bySampleSource": {},
    }
    for field, output in (
        ("outcome", "byOutcome"),
        ("findingCode", "byFindingCode"),
        ("sampleSource", "bySampleSource"),
    ):
        for value in sorted({row[field] for row in rows}):
            report[output][value] = _group_summary([row for row in rows if row[field] == value])
    return report


def load_records(path):
    rows = []
    seen_ids = set()
    with open(path, encoding="utf-8") as source:
        for line_number, line in enumerate(source, 1):
            if len(line.encode("utf-8")) > MAX_LINE_BYTES:
                raise ValueError(f"{path}:{line_number}: line exceeds {MAX_LINE_BYTES} bytes")
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            try:
                parsed = json.loads(line, object_pairs_hook=_unique_object)
            except (json.JSONDecodeError, ValueError) as error:
                raise ValueError(f"{path}:{line_number}: invalid JSON: {error}") from error
            row = validate_row(parsed, line_number)
            if row["id"] in seen_ids:
                raise ValueError(f"{path}:{line_number}: duplicate evaluation ID")
            seen_ids.add(row["id"])
            rows.append(row)
            if len(rows) > MAX_RECORDS:
                raise ValueError(f"{path}: exceeds {MAX_RECORDS} records")
    return rows


def write_json(path, report):
    output = Path(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent,
            prefix=f".{output.name}.", delete=False,
        ) as target:
            temporary_path = target.name
            os.chmod(temporary_path, 0o600)
            json.dump(report, target, ensure_ascii=False, indent=2, allow_nan=False)
            target.write("\n")
        os.replace(temporary_path, output)
        os.chmod(output, 0o600)
    except OSError:
        if temporary_path and os.path.exists(temporary_path):
            os.unlink(temporary_path)
        raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", help="JSONL from export-optimization-shadow-evaluations.py")
    parser.add_argument("--output", help="optional private JSON report path")
    args = parser.parse_args(argv)
    try:
        report = build_report(load_records(args.input))
        rendered = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False)
        if args.output:
            write_json(args.output, report)
        else:
            print(rendered)
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
