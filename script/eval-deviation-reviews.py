#!/usr/bin/env python3
"""Describe reviewed attribution accuracy; project labels do not establish M3 blind-review acceptance."""

import argparse
import json
import math
import sys

CALIBRATION_BIN_COUNT = 5
MIN_CALIBRATION_BIN_SAMPLES = 10
DEFAULT_MINIMUM_RUNS = 100
DEFAULT_TARGET_ACCURACY = 0.75
MAX_RECORDS = 100_000
MAX_LINE_BYTES = 65_536
MAX_I64 = 9_223_372_036_854_775_807
INVENTORY_FIELDS = {"caseId", "decisionRunId", "status", "confidence"}
REVIEW_FIELDS = INVENTORY_FIELDS | {"attributedRule", "label"}


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def validate_records(records, *, inventory=False):
    """Validate IDs for both file and programmatic callers before counting runs."""
    fields = INVENTORY_FIELDS if inventory else REVIEW_FIELDS
    seen_ids = set()
    seen_runs = set()
    normalized = []
    for index, record in enumerate(records, 1):
        if index > MAX_RECORDS:
            raise ValueError(f"input exceeds {MAX_RECORDS} records")
        if not isinstance(record, dict) or set(record) != fields:
            raise ValueError(f"row {index}: fields must be exactly {', '.join(sorted(fields))}")
        case_id = record["caseId"]
        if not isinstance(case_id, str) or not case_id.strip() or len(case_id) > 255:
            raise ValueError(f"row {index}: caseId must be a non-empty string up to 255 characters")
        if case_id in seen_ids:
            raise ValueError(f"row {index}: duplicate caseId")
        run_id = record["decisionRunId"]
        if type(run_id) is not int or not 0 < run_id <= MAX_I64:
            raise ValueError(f"row {index}: decisionRunId must be a positive 64-bit integer")
        if run_id in seen_runs:
            raise ValueError(f"row {index}: duplicate decisionRunId; one attribution case per run is required")
        status = record["status"]
        if not isinstance(status, str) or status not in ("candidate", "insufficient_evidence"):
            raise ValueError(f"row {index}: unsupported attribution status")
        confidence = record["confidence"]
        if (not isinstance(confidence, (int, float)) or isinstance(confidence, bool)
                or not 0 <= confidence <= 1 or not math.isfinite(confidence)):
            raise ValueError(f"row {index}: confidence must be between 0 and 1")
        if not inventory:
            label = record["label"]
            if not isinstance(label, str) or label not in ("correct", "incorrect", "inconclusive"):
                raise ValueError(f"row {index}: unsupported review label")
            if status == "insufficient_evidence" and label != "inconclusive":
                raise ValueError(f"row {index}: insufficient evidence must be inconclusive")
            rule = record["attributedRule"]
            if rule is not None and (not isinstance(rule, str) or len(rule) > 4096):
                raise ValueError(f"row {index}: attributedRule must be a string up to 4096 characters or null")
            if status == "candidate" and (not isinstance(rule, str) or not rule.strip()):
                raise ValueError(f"row {index}: candidate attribution requires attributedRule")
        normalized.append(record.copy())
        seen_ids.add(case_id)
        seen_runs.add(run_id)
    return normalized


def _load_records(path, *, inventory=False):
    records = []
    with open(path, "rb") as source:
        line_number = 0
        while True:
            line = source.readline(MAX_LINE_BYTES + 1)
            if not line:
                break
            line_number += 1
            if len(line) > MAX_LINE_BYTES:
                raise ValueError(f"{path}:{line_number}: line exceeds {MAX_LINE_BYTES} bytes")
            try:
                line = line.decode("utf-8").strip()
            except UnicodeDecodeError as error:
                raise ValueError(f"{path}:{line_number}: input must be UTF-8") from error
            if not line or line.startswith("#"):
                continue
            try:
                record = json.loads(line, object_pairs_hook=_unique_object)
            except (json.JSONDecodeError, ValueError) as error:
                raise ValueError(f"{path}:{line_number}: invalid JSON: {error}") from error
            records.append(record)
            if len(records) > MAX_RECORDS:
                raise ValueError(f"{path}: exceeds {MAX_RECORDS} records")
    return validate_records(records, inventory=inventory)


def load_cases(path):
    return _load_records(path)


def load_inventory(path):
    return _load_records(path, inventory=True)


def wilson_interval(successes, total, z=1.959963984540054):
    if total <= 0:
        return None
    rate = successes / total
    denominator = 1 + z * z / total
    center = (rate + z * z / (2 * total)) / denominator
    margin = z * math.sqrt(rate * (1 - rate) / total + z * z / (4 * total * total)) / denominator
    return {"lower": round(max(0.0, center - margin), 6), "upper": round(min(1.0, center + margin), 6)}


def evaluate(cases, *, inventory=None, minimum_runs=DEFAULT_MINIMUM_RUNS,
             target_accuracy=DEFAULT_TARGET_ACCURACY):
    if type(minimum_runs) is not int or minimum_runs < 1:
        raise ValueError("minimum_runs must be a positive integer")
    if (not isinstance(target_accuracy, (int, float)) or isinstance(target_accuracy, bool)
            or not 0 <= target_accuracy <= 1 or not math.isfinite(target_accuracy)):
        raise ValueError("target_accuracy must be a finite ratio between 0 and 1")
    cases = validate_records(cases)
    if inventory is not None:
        inventory = validate_records(inventory, inventory=True)
        by_id = {record["caseId"]: record for record in inventory}
        for case in cases:
            supplied = by_id.get(case["caseId"])
            if supplied is None:
                raise ValueError("reviewed case is absent from the supplied inventory")
            if any(case[field] != supplied[field] for field in INVENTORY_FIELDS):
                raise ValueError("review metadata does not match the supplied inventory snapshot")

    candidates = [case for case in cases if case["status"] == "candidate"]
    correct = sum(case["label"] == "correct" for case in candidates)
    incorrect = sum(case["label"] == "incorrect" for case in candidates)
    conclusive = correct + incorrect
    calibration_cases = [
        case for case in candidates if case["label"] in ("correct", "incorrect")
    ]
    bins = []
    for bin_index in range(CALIBRATION_BIN_COUNT):
        lower_bound = bin_index / CALIBRATION_BIN_COUNT
        upper_bound = (bin_index + 1) / CALIBRATION_BIN_COUNT
        members = [
            case for case in calibration_cases
            if lower_bound <= case["confidence"]
            and (case["confidence"] < upper_bound or bin_index == CALIBRATION_BIN_COUNT - 1)
        ]
        count = len(members)
        mean_confidence = (
            sum(case["confidence"] for case in members) / count if count else None
        )
        observed_accuracy = (
            sum(case["label"] == "correct" for case in members) / count if count else None
        )
        gap = abs(mean_confidence - observed_accuracy) if count else None
        bins.append({
            "lowerBound": round(lower_bound, 2),
            "upperBound": round(upper_bound, 2),
            "upperBoundInclusive": bin_index == CALIBRATION_BIN_COUNT - 1,
            "samples": count,
            "meanConfidence": round(mean_confidence, 4) if count else None,
            "observedAccuracy": round(observed_accuracy, 4) if count else None,
            "absoluteGap": round(gap, 4) if count else None,
            "smallSample": count < MIN_CALIBRATION_BIN_SAMPLES,
        })
    brier_score = (
        sum(
            (case["confidence"] - (1 if case["label"] == "correct" else 0)) ** 2
            for case in calibration_cases
        ) / conclusive
        if conclusive else None
    )
    expected_calibration_error = (
        sum(item["samples"] * item["absoluteGap"] for item in bins if item["samples"])
        / conclusive
        if conclusive else None
    )
    return {
        "scope": "human-reviewed candidate attributions only",
        "reviewedCases": len(cases),
        "candidateCases": len(candidates),
        "correct": correct,
        "incorrect": incorrect,
        "inconclusive": len(cases) - conclusive,
        "conclusiveCases": conclusive,
        "top1Accuracy": round(correct / conclusive, 4) if conclusive else None,
        "conclusiveCoverage": round(conclusive / len(cases), 4) if cases else 0.0,
        "uniqueDecisionRunCount": len(cases),
        "top1AccuracyWilson95": wilson_interval(correct, conclusive),
        "candidateAccuracyAssessment": {
            "minimumConclusiveRuns": minimum_runs,
            "targetAccuracy": target_accuracy,
            "conclusiveRuns": conclusive,
            "status": (
                "insufficient_samples" if conclusive < minimum_runs
                else "target_met" if correct / conclusive >= target_accuracy
                else "target_not_met"
            ),
            "scope": "numerical_check_of_mutable_project_review_labels",
        },
        "m3Acceptance": {
            "status": "not_established",
            "passed": False,
            "independentBlindReviewEvidence": False,
            "requiredIndependentConclusiveRuns": DEFAULT_MINIMUM_RUNS,
            "requiredTop1Accuracy": DEFAULT_TARGET_ACCURACY,
            "reason": "Project review labels are mutable and do not establish independent blinded expert review or representative sampling.",
        },
        "samplingFrame": {
            "inventoryProvided": inventory is not None,
            "scope": "supplied_attribution_inventory" if inventory is not None else "reviewed_cases_only",
            "attributionRuns": len(inventory) if inventory is not None else None,
            "reviewedRuns": len(cases),
            "unreviewedRuns": len(inventory) - len(cases) if inventory is not None else None,
            "reviewCoverage": round(len(cases) / len(inventory), 4) if inventory else None,
            "conclusiveCoverage": round(conclusive / len(inventory), 4) if inventory else None,
            "unreviewedCountKnown": inventory is not None,
            "completenessVerified": False,
        },
        "confidenceCalibration": {
            "method": "equal_width_bins",
            "binCount": CALIBRATION_BIN_COUNT,
            "minimumSamplesPerBin": MIN_CALIBRATION_BIN_SAMPLES,
            "calibrationCases": conclusive,
            "brierScore": round(brier_score, 4) if brier_score is not None else None,
            "expectedCalibrationError": (
                round(expected_calibration_error, 4)
                if expected_calibration_error is not None else None
            ),
            "bins": bins,
        },
        "limitation": "This metric covers reviewed candidate attributions only; it does not estimate attribution recall or quality on unreviewed cases. Wilson intervals do not correct selection bias or correlated runs. A supplied inventory is not proof of complete or representative business coverage, and numerical targets do not establish M3 acceptance.",
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reviewed_set", help="JSONL exported by export-deviation-reviews.py")
    parser.add_argument("--output", help="write JSON report to this path instead of stdout")
    parser.add_argument("--inventory", help="optional JSONL attribution inventory, including unreviewed runs")
    parser.add_argument("--minimum-runs", type=int, default=DEFAULT_MINIMUM_RUNS)
    parser.add_argument("--target-accuracy", type=float, default=DEFAULT_TARGET_ACCURACY)
    args = parser.parse_args(argv)
    try:
        report = evaluate(
            load_cases(args.reviewed_set),
            inventory=load_inventory(args.inventory) if args.inventory is not None else None,
            minimum_runs=args.minimum_runs,
            target_accuracy=args.target_accuracy,
        )
    except (OSError, ValueError) as error:
        parser.error(str(error))
    rendered = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
    if args.output:
        with open(args.output, "w", encoding="utf-8") as target:
            target.write(rendered)
    else:
        sys.stdout.write(rendered)


if __name__ == "__main__":
    main()
