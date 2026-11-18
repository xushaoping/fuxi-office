#!/usr/bin/env python3
"""Evaluate human Tester-proposal acceptance against a frozen candidate inventory."""

import argparse
from collections import Counter, defaultdict
import json
import math
import re
import sys

MAX_RECORDS = 100_000
MAX_LINE_BYTES = 16_384
MAX_FILE_BYTES = 32 * 1024 * 1024
MAX_I64 = 9_223_372_036_854_775_807
CASE_FIELDS = {"projectId", "versionId", "reportDigest", "proposalId"}
REVIEW_FIELDS = CASE_FIELDS | {"reviewer", "label"}
LABELS = {"accepted", "rejected", "inconclusive"}


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def _reject_constant(_value):
    raise ValueError("non-finite JSON number")


def validate_row(row, index, review=False):
    expected = REVIEW_FIELDS if review else CASE_FIELDS
    if not isinstance(row, dict) or set(row) != expected:
        raise ValueError(f"record {index}: expected exactly the documented {'review' if review else 'inventory'} fields")
    for field in ("projectId", "versionId"):
        if type(row[field]) is not int or not 0 < row[field] <= MAX_I64:
            raise ValueError(f"record {index}: {field} must be a positive signed 64-bit integer")
    if not isinstance(row["reportDigest"], str) or not re.fullmatch(r"[0-9a-f]{64}", row["reportDigest"]):
        raise ValueError(f"record {index}: reportDigest must be a lowercase SHA-256 digest")
    if not isinstance(row["proposalId"], str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,63}", row["proposalId"]):
        raise ValueError(f"record {index}: proposalId must be a stable identifier up to 64 ASCII characters")
    if review:
        reviewer = row["reviewer"]
        if (not isinstance(reviewer, str) or not reviewer or reviewer != reviewer.strip()
                or len(reviewer.encode("utf-8")) > 255
                or any(ord(character) < 32 or ord(character) == 127 for character in reviewer)):
            raise ValueError(f"record {index}: reviewer must be a trimmed identifier up to 255 UTF-8 bytes without control characters")
        if not isinstance(row["label"], str) or row["label"] not in LABELS:
            raise ValueError(f"record {index}: unsupported human review label")
    return row


def case_key(row):
    return (row["projectId"], row["versionId"], row["reportDigest"], row["proposalId"])


def load_records(path, review=False):
    """Bound bytes before decoding, including blank/comment lines."""
    rows = []
    consumed = 0
    with open(path, "rb") as source:
        line_number = 0
        while True:
            raw = source.readline(MAX_LINE_BYTES + 1)
            if not raw:
                break
            line_number += 1
            consumed += len(raw)
            if len(raw) > MAX_LINE_BYTES:
                raise ValueError(f"line {line_number}: exceeds {MAX_LINE_BYTES} bytes")
            if consumed > MAX_FILE_BYTES:
                raise ValueError(f"input exceeds {MAX_FILE_BYTES} bytes")
            try:
                line = raw.decode("utf-8").strip()
                if not line or line.startswith("#"):
                    continue
                row = json.loads(line, object_pairs_hook=_unique_object, parse_constant=_reject_constant)
            except (UnicodeDecodeError, json.JSONDecodeError, ValueError, RecursionError) as error:
                raise ValueError(f"line {line_number}: invalid JSONL ({type(error).__name__})") from error
            rows.append(validate_row(row, line_number, review=review))
            if len(rows) > MAX_RECORDS:
                raise ValueError(f"input exceeds {MAX_RECORDS} records")
    return rows


def wilson_interval(successes, total, z=1.959963984540054):
    if not total:
        return None
    rate = successes / total
    denominator = 1 + z * z / total
    center = (rate + z * z / (2 * total)) / denominator
    margin = z * math.sqrt(rate * (1 - rate) / total + z * z / (4 * total * total)) / denominator
    return {"lower": round(max(0.0, center - margin), 6), "upper": round(min(1.0, center + margin), 6)}


def _validate_ratio(name, value):
    if (not isinstance(value, (int, float)) or isinstance(value, bool)
            or not math.isfinite(value) or not 0 <= value <= 1):
        raise ValueError(f"{name} must be a finite ratio from 0 to 1")


def build_report(inventory, reviews, minimum_cases=100, target_rate=0.70, minimum_coverage=0.90):
    if type(minimum_cases) is not int or not 1 <= minimum_cases <= MAX_RECORDS:
        raise ValueError(f"minimum_cases must be an integer from 1 to {MAX_RECORDS}")
    _validate_ratio("target_rate", target_rate)
    _validate_ratio("minimum_coverage", minimum_coverage)
    if len(inventory) > MAX_RECORDS or len(reviews) > MAX_RECORDS:
        raise ValueError(f"input exceeds {MAX_RECORDS} records")

    candidates = set()
    for index, row in enumerate(inventory, 1):
        key = case_key(validate_row(row, index))
        if key in candidates:
            raise ValueError(f"duplicate inventory case at record {index}")
        candidates.add(key)
    if not candidates:
        raise ValueError("candidate inventory must not be empty")

    by_case = defaultdict(Counter)
    seen_votes = set()
    for index, row in enumerate(reviews, 1):
        key = case_key(validate_row(row, index, review=True))
        if key not in candidates:
            raise ValueError(f"review record {index}: case is absent from the frozen inventory")
        vote = (*key, row["reviewer"])
        if vote in seen_votes:
            raise ValueError(f"duplicate reviewer/case pair at review record {index}")
        seen_votes.add(vote)
        by_case[key][row["label"]] += 1

    counts = {
        "candidate": len(candidates), "reviewed": len(by_case),
        "unreviewed": len(candidates) - len(by_case),
        "determinate": 0, "accepted": 0, "rejected": 0,
        "inconclusive": 0, "tied": 0, "onlyInconclusive": 0,
    }
    for votes in by_case.values():
        positive, negative = votes["accepted"], votes["rejected"]
        if positive == negative:
            counts["inconclusive"] += 1
            counts["tied" if positive else "onlyInconclusive"] += 1
        else:
            counts["determinate"] += 1
            counts["accepted" if positive > negative else "rejected"] += 1

    rate = counts["accepted"] / counts["determinate"] if counts["determinate"] else None
    coverage = counts["determinate"] / counts["candidate"]
    missing = []
    if counts["determinate"] < minimum_cases:
        missing.append("minimum_determinate_cases")
    if coverage < minimum_coverage:
        missing.append("minimum_determinate_coverage")
    return {
        "schemaVersion": 1,
        "reviewCount": len(reviews),
        "caseCounts": counts,
        "coverage": {
            "reviewedRate": round(counts["reviewed"] / counts["candidate"], 6),
            "determinateRate": round(coverage, 6),
            "minimumDeterminateRate": minimum_coverage,
        },
        "acceptance": {
            "rate": round(rate, 6) if rate is not None else None,
            "acceptedFractionOfAllCandidates": round(counts["accepted"] / counts["candidate"], 6),
            "wilson95": wilson_interval(counts["accepted"], counts["determinate"]),
            "targetRate": target_rate,
            "minimumCases": minimum_cases,
            "status": ("insufficient_samples" if missing else "target_met" if rate >= target_rate else "target_not_met"),
            "insufficiencyReasons": missing,
        },
        "reviewersAndCaseIdsOmitted": True,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inventory", help="frozen inventory JSONL with projectId, versionId, reportDigest, proposalId")
    parser.add_argument("reviews", help="human review JSONL: inventory case fields plus reviewer and label")
    parser.add_argument("--minimum-cases", type=int, default=100)
    parser.add_argument("--target-rate", type=float, default=0.70)
    parser.add_argument("--minimum-coverage", type=float, default=0.90)
    args = parser.parse_args(argv)
    try:
        report = build_report(
            load_records(args.inventory), load_records(args.reviews, review=True),
            minimum_cases=args.minimum_cases,
            target_rate=args.target_rate,
            minimum_coverage=args.minimum_coverage,
        )
        print(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False))
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
