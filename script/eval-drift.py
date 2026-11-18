#!/usr/bin/env python3
"""Evaluate drift findings against a human-labeled JSONL replay set."""

import argparse
import datetime
import itertools
import json
import math
import sys

DRIFT_SIGNAL_CODES = {
    "INPUT_DISTRIBUTION_DRIFT",
    "INPUT_FIELD_MISSINGNESS_DRIFT",
}
WILSON_Z_95 = 1.959963984540054


def parse_number_list(value, name, minimum, maximum):
    try:
        numbers = [float(part.strip()) for part in value.split(",")]
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"{name} must be comma-separated numbers") from error
    if not numbers or any(not math.isfinite(number) or number < minimum or number > maximum for number in numbers):
        raise argparse.ArgumentTypeError(f"{name} values must be between {minimum} and {maximum}")
    return sorted(set(numbers))


def parse_integer_list(value):
    try:
        numbers = [int(part.strip()) for part in value.split(",")]
    except ValueError as error:
        raise argparse.ArgumentTypeError("minimum run counts must be comma-separated integers") from error
    if not numbers or any(number < 1 for number in numbers):
        raise argparse.ArgumentTypeError("minimum run counts must be positive integers")
    return sorted(set(numbers))


def load_cases(path):
    cases = []
    seen_ids = set()
    with open(path, encoding="utf-8") as source:
        for line_number, line in enumerate(source, 1):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as error:
                raise ValueError(f"{path}:{line_number}: invalid JSON: {error.msg}") from error
            if not isinstance(record, dict):
                raise ValueError(f"{path}:{line_number}: each row must be a JSON object")
            case_id = record.get("caseId")
            label = record.get("label")
            report = record.get("report")
            if not isinstance(case_id, str) or not case_id.strip():
                raise ValueError(f"{path}:{line_number}: caseId must be a non-empty string")
            if case_id in seen_ids:
                raise ValueError(f"{path}:{line_number}: duplicate caseId {case_id!r}")
            if label not in ("drift", "normal"):
                raise ValueError(f"{path}:{line_number}: label must be 'drift' or 'normal'")
            for field in ("projectId", "sourceVersionId"):
                value = record.get(field)
                if value is not None and (type(value) is not int or value <= 0):
                    raise ValueError(f"{path}:{line_number}: {field} must be a positive integer when present")
            if not isinstance(report, dict) or not isinstance(report.get("summary"), dict):
                raise ValueError(f"{path}:{line_number}: report.summary is required")
            summary = report["summary"]
            for field in ("totalRuns", "conclusiveOutcomeRuns"):
                if type(summary.get(field)) is not int or summary[field] < 0:
                    raise ValueError(f"{path}:{line_number}: summary.{field} must be a non-negative integer")
            for field in ("failureRate", "mismatchRate"):
                value = summary.get(field)
                if not isinstance(value, (int, float)) or not math.isfinite(value) or not 0 <= value <= 1:
                    raise ValueError(f"{path}:{line_number}: summary.{field} must be between 0 and 1")
            findings = report.get("findings", [])
            if not isinstance(findings, list) or any(not isinstance(item, dict) or not isinstance(item.get("code"), str) for item in findings):
                raise ValueError(f"{path}:{line_number}: report.findings must contain finding codes")
            raw_signals = report.get("driftSignals", [])
            if not isinstance(raw_signals, list):
                raise ValueError(f"{path}:{line_number}: report.driftSignals must be an array")
            drift_signals = []
            seen_signals = set()
            for signal in raw_signals:
                if not isinstance(signal, dict) or signal.get("code") not in DRIFT_SIGNAL_CODES:
                    raise ValueError(f"{path}:{line_number}: unsupported drift signal")
                element = signal.get("element")
                metric_value = signal.get("metricValue")
                p_value = signal.get("pValue")
                baseline_samples = signal.get("baselineSampleCount")
                recent_samples = signal.get("recentSampleCount")
                if not isinstance(element, str) or not element.strip():
                    raise ValueError(f"{path}:{line_number}: drift signal element must be non-empty")
                if (not isinstance(metric_value, (int, float)) or isinstance(metric_value, bool)
                        or not math.isfinite(metric_value) or metric_value < 0):
                    raise ValueError(f"{path}:{line_number}: drift signal metricValue must be finite and non-negative")
                if (not isinstance(p_value, (int, float)) or isinstance(p_value, bool)
                        or not math.isfinite(p_value) or not 0 <= p_value <= 1):
                    raise ValueError(f"{path}:{line_number}: drift signal pValue must be between 0 and 1")
                for name, value in (("baselineSampleCount", baseline_samples),
                                    ("recentSampleCount", recent_samples)):
                    if type(value) is not int or value < 0:
                        raise ValueError(f"{path}:{line_number}: drift signal {name} must be a non-negative integer")
                signal_key = (signal["code"], element)
                if signal_key in seen_signals:
                    raise ValueError(f"{path}:{line_number}: duplicate drift signal {signal_key!r}")
                seen_signals.add(signal_key)
                drift_signals.append(signal)
            cases.append({
                "caseId": case_id,
                "label": label,
                "report": report,
                "codes": {item["code"] for item in findings},
                "driftSignals": drift_signals,
                "hasDriftSignals": "driftSignals" in report,
                "scanDay": record.get("scanDay"),
                "projectId": record.get("projectId"),
                "sourceVersionId": record.get("sourceVersionId"),
            })
            seen_ids.add(case_id)
    if not cases:
        raise ValueError("the labeled drift replay set has no cases")
    return cases


def score(cases, predictions):
    if len(cases) != len(predictions):
        raise ValueError("prediction count must match case count")
    tp = fp = tn = fn = 0
    for case, predicted in zip(cases, predictions):
        positive = case["label"] == "drift"
        if predicted and positive:
            tp += 1
        elif predicted:
            fp += 1
        elif positive:
            fn += 1
        else:
            tn += 1
    precision = tp / (tp + fp) if tp + fp else 0.0
    recall = tp / (tp + fn) if tp + fn else 0.0
    false_positive_rate = fp / (fp + tn) if fp + tn else 0.0
    return {
        "cases": len(cases),
        "driftCases": tp + fn,
        "normalCases": tn + fp,
        "truePositive": tp,
        "falsePositive": fp,
        "trueNegative": tn,
        "falseNegative": fn,
        "precision": round(precision, 4),
        "precisionDenominator": tp + fp,
        "precisionWilson95": wilson_interval(tp, tp + fp),
        "recall": round(recall, 4),
        "recallDenominator": tp + fn,
        "recallWilson95": wilson_interval(tp, tp + fn),
        "f1": round(2 * precision * recall / (precision + recall), 4) if precision + recall else 0.0,
        "falsePositiveRate": round(false_positive_rate, 4),
        "falsePositiveRateDenominator": fp + tn,
        "falsePositiveRateWilson95": wilson_interval(fp, fp + tn),
    }


def wilson_interval(successes, trials, z=WILSON_Z_95):
    if trials <= 0:
        return None
    proportion = successes / trials
    z_squared = z * z
    denominator = 1 + z_squared / trials
    center = (proportion + z_squared / (2 * trials)) / denominator
    margin = (z / denominator) * math.sqrt(
        proportion * (1 - proportion) / trials + z_squared / (4 * trials * trials)
    )
    return [round(max(0.0, center - margin), 4), round(min(1.0, center + margin), 4)]


def predict_threshold_sweep(case, min_runs, mismatch_threshold, failure_threshold,
                            psi_threshold=0.25, missing_rate_gap_threshold=0.15):
    summary = case["report"]["summary"]
    codes = case["codes"] - {"HIGH_MISMATCH_RATE", "HIGH_FAILURE_RATE"}
    if case.get("hasDriftSignals"):
        codes -= DRIFT_SIGNAL_CODES
    if summary["totalRuns"] >= min_runs and summary["failureRate"] >= failure_threshold:
        codes.add("HIGH_FAILURE_RATE")
    if (summary["conclusiveOutcomeRuns"] >= min_runs
            and summary["mismatchRate"] >= mismatch_threshold):
        codes.add("HIGH_MISMATCH_RATE")
    if case.get("hasDriftSignals"):
        family_size = len(case["driftSignals"])
        for signal in case["driftSignals"]:
            adjusted_p = min(1.0, signal["pValue"] * max(1, family_size))
            if adjusted_p > 0.05:
                continue
            if (signal["code"] == "INPUT_DISTRIBUTION_DRIFT"
                    and signal["metricValue"] >= psi_threshold):
                codes.add(signal["code"])
            elif (signal["code"] == "INPUT_FIELD_MISSINGNESS_DRIFT"
                    and signal["metricValue"] >= missing_rate_gap_threshold):
                codes.add(signal["code"])
    return bool(codes)


def evaluate(cases, min_runs_values, mismatch_values, failure_values,
             psi_values=None, missing_rate_gap_values=None):
    psi_values = psi_values or [0.25]
    missing_rate_gap_values = missing_rate_gap_values or [0.15]
    codes = sorted({code for case in cases for code in case["codes"]})
    per_signal = {
        code: score(cases, [code in case["codes"] for case in cases])
        for code in codes
    }
    current_predictions = [bool(case["codes"]) for case in cases]
    sweep = []
    for min_runs, mismatch_threshold, failure_threshold, psi_threshold, missing_threshold in itertools.product(
            min_runs_values, mismatch_values, failure_values, psi_values, missing_rate_gap_values):
        predictions = [
            predict_threshold_sweep(
                case, min_runs, mismatch_threshold, failure_threshold,
                psi_threshold, missing_threshold
            )
            for case in cases
        ]
        sweep.append({
            "minRuns": min_runs,
            "mismatchRateThreshold": mismatch_threshold,
            "failureRateThreshold": failure_threshold,
            "psiThreshold": psi_threshold,
            "missingRateGapThreshold": missing_threshold,
            **score(cases, predictions),
        })
    sweep.sort(key=lambda row: (row["f1"], row["precision"], row["recall"]), reverse=True)
    return {
        "caseCount": len(cases),
        "labelCounts": {
            "drift": sum(case["label"] == "drift" for case in cases),
            "normal": sum(case["label"] == "normal" for case in cases),
        },
        "currentFindings": score(cases, current_predictions),
        "perSignal": per_signal,
        "thresholdSweep": sweep,
        "thresholdSelectionScope": "same labeled cases; use temporal holdout for separate evaluation",
        "thresholdSweepScope": [
            "HIGH_FAILURE_RATE", "HIGH_MISMATCH_RATE",
            "INPUT_DISTRIBUTION_DRIFT", "INPUT_FIELD_MISSINGNESS_DRIFT",
        ],
        "note": "Distribution thresholds replay only privacy-limited driftSignals stored in each report; older reports without driftSignals keep their recorded distribution findings.",
    }


def parse_utc_timestamp(raw, field):
    try:
        timestamp = datetime.datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except (AttributeError, TypeError, ValueError) as error:
        raise ValueError(f"{field} must be a timezone-aware ISO timestamp") from error
    if timestamp.tzinfo is None:
        raise ValueError(f"{field} must be a timezone-aware ISO timestamp")
    return timestamp.astimezone(datetime.timezone.utc)


def evaluate_temporal_holdout(cases, holdout_from, min_runs_values, mismatch_values,
                              failure_values, psi_values, missing_rate_gap_values):
    try:
        cutoff = datetime.date.fromisoformat(holdout_from)
    except (TypeError, ValueError) as error:
        raise ValueError("holdout-from must be an ISO date (YYYY-MM-DD)") from error
    dated_cases = []
    for case in cases:
        for field in ("projectId", "sourceVersionId"):
            value = case.get(field)
            if value is not None and (type(value) is not int or value <= 0):
                raise ValueError(f"{field} must be a positive integer when present")
        try:
            day = datetime.date.fromisoformat(case.get("scanDay"))
        except (TypeError, ValueError) as error:
            raise ValueError("temporal holdout requires scanDay on every case") from error
        report = case["report"]
        start = parse_utc_timestamp(report.get("windowStart"), "report.windowStart")
        end = parse_utc_timestamp(report.get("generatedAt"), "report.generatedAt")
        if start >= end or end.date() != day:
            raise ValueError("report window must be non-empty and generatedAt must match scanDay in UTC")
        dated_cases.append((case, day, start, end))
    holdout = [item for item in dated_cases if item[1] >= cutoff]
    if not holdout:
        raise ValueError("temporal holdout has no evaluation cases")
    earliest_holdout_start = min(item[2] for item in holdout)
    training = [item for item in dated_cases
                if item[1] < cutoff and item[3] < earliest_holdout_start]
    if not training:
        raise ValueError("no training cases remain after purging overlapping report windows")
    training_cases = [item[0] for item in training]
    holdout_cases = [item[0] for item in holdout]
    for name, subset in (("training", training_cases), ("holdout", holdout_cases)):
        if {case["label"] for case in subset} != {"drift", "normal"}:
            raise ValueError(f"{name} set requires both drift and normal labels")
    training_report = evaluate(training_cases, min_runs_values, mismatch_values,
                               failure_values, psi_values, missing_rate_gap_values)
    selected = training_report["thresholdSweep"][0]
    keys = ("minRuns", "mismatchRateThreshold", "failureRateThreshold",
            "psiThreshold", "missingRateGapThreshold")
    thresholds = {key: selected[key] for key in keys}
    predictions = [predict_threshold_sweep(case, *(thresholds[key] for key in keys))
                   for case in holdout_cases]
    strata = {}
    for index, case in enumerate(holdout_cases):
        project_id = case.get("projectId")
        source_version_id = case.get("sourceVersionId")
        key = (project_id, source_version_id)
        strata.setdefault(key, {"cases": [], "predictions": []})
        strata[key]["cases"].append(case)
        strata[key]["predictions"].append(predictions[index])
    by_project_and_version = [
        {
            "projectId": key[0],
            "sourceVersionId": key[1],
            "scope": "unattributed" if key == (None, None) else "project and source version",
            **score(group["cases"], group["predictions"]),
        }
        for key, group in sorted(
            strata.items(),
            key=lambda item: tuple(-1 if value is None else value for value in item[0]),
        )
    ]
    return {
        "scope": "temporal holdout with overlapping training windows purged",
        "holdoutFrom": cutoff.isoformat(),
        "trainingCases": len(training_cases),
        "holdoutCases": len(holdout_cases),
        "purgedCases": len(cases) - len(training_cases) - len(holdout_cases),
        "latestTrainingEnd": max(item[3] for item in training).isoformat(),
        "earliestHoldoutStart": earliest_holdout_start.isoformat(),
        "selectedThresholds": thresholds,
        "trainingMetrics": {key: value for key, value in selected.items() if key not in keys},
        "holdoutMetrics": score(holdout_cases, predictions),
        "holdoutMetricsByProjectAndVersion": by_project_and_version,
        "holdoutCurrentFindings": score(holdout_cases, [bool(case["codes"]) for case in holdout_cases]),
        "limitation": "Thresholds are selected only on training labels. Holdout windows may overlap each other; intervals do not correct sample correlation or label selection bias.",
    }


def evaluate_reviewed_events(cases):
    codes = sorted({code for case in cases for code in case["codes"]})

    def confirmation_metrics(items):
        confirmed = sum(case["label"] == "drift" for case in items)
        false_positives = sum(case["label"] == "normal" for case in items)
        reviewed = confirmed + false_positives
        return {
            "reviewedEvents": reviewed,
            "confirmedDrift": confirmed,
            "falsePositive": false_positives,
            "confirmationRate": round(confirmed / reviewed, 4) if reviewed else 0.0,
            "confirmationRateWilson95": wilson_interval(confirmed, reviewed),
            "falseDiscoveryRate": round(false_positives / reviewed, 4) if reviewed else 0.0,
            "falseDiscoveryRateWilson95": wilson_interval(false_positives, reviewed),
        }

    return {
        "scope": "reviewed triggered events only",
        "reviewedEventCount": len(cases),
        "overall": confirmation_metrics(cases),
        "perSignal": {
            code: confirmation_metrics([case for case in cases if code in case["codes"]])
            for code in codes
        },
        "limitation": "This sample contains detected events only; it cannot estimate recall or false positives in non-triggered windows.",
    }
def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("labeled_set", help="JSONL rows: caseId, label (drift|normal), and report")
    parser.add_argument("--min-runs", default="5,10,20", type=parse_integer_list)
    parser.add_argument("--mismatch-thresholds", default="0.1,0.2,0.3", type=lambda value: parse_number_list(value, "mismatch thresholds", 0, 1))
    parser.add_argument("--failure-thresholds", default="0.1,0.2,0.3", type=lambda value: parse_number_list(value, "failure thresholds", 0, 1))
    parser.add_argument("--psi-thresholds", default="0.25,0.35,0.5", type=lambda value: parse_number_list(value, "PSI thresholds", 0, 10))
    parser.add_argument("--missing-rate-gap-thresholds", default="0.15,0.25,0.35", type=lambda value: parse_number_list(value, "missing rate gap thresholds", 0, 1))
    parser.add_argument("--output", help="write JSON report to this path instead of stdout")
    parser.add_argument("--holdout-from", help="select thresholds on earlier non-overlapping windows and evaluate from this ISO date")
    parser.add_argument(
        "--reviewed-events",
        action="store_true",
        help="report confirmation and false discovery rates for reviewed triggered events only",
    )
    args = parser.parse_args()
    if args.holdout_from and args.reviewed_events:
        parser.error("--holdout-from requires full windows, not --reviewed-events")
    combinations = (len(args.min_runs) * len(args.mismatch_thresholds)
                    * len(args.failure_thresholds) * len(args.psi_thresholds)
                    * len(args.missing_rate_gap_thresholds))
    if combinations > 1000:
        parser.error("threshold sweep exceeds 1000 combinations")
    try:
        cases = load_cases(args.labeled_set)
        if args.reviewed_events:
            report = evaluate_reviewed_events(cases)
        elif args.holdout_from:
            report = evaluate_temporal_holdout(
                cases, args.holdout_from, args.min_runs, args.mismatch_thresholds,
                args.failure_thresholds, args.psi_thresholds, args.missing_rate_gap_thresholds
            )
        else:
            report = evaluate(cases, args.min_runs,
                              args.mismatch_thresholds, args.failure_thresholds,
                              args.psi_thresholds, args.missing_rate_gap_thresholds)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        with open(args.output, "w", encoding="utf-8") as target:
            target.write(rendered)
    else:
        sys.stdout.write(rendered)


if __name__ == "__main__":
    main()
