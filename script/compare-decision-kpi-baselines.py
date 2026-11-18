#!/usr/bin/env python3
"""Compare privacy-limited decision analytics KPI snapshots."""

import argparse
import json
import math
import os
from pathlib import Path
import sys
import tempfile

COUNTS = (
    "totalRuns",
    "succeededRuns",
    "failedRuns",
    "outcomeReportedRuns",
    "conclusiveOutcomeRuns",
    "confirmedRuns",
    "mismatchRuns",
    "inconclusiveRuns",
)
RATES = {
    "failureRate": ("failedRuns", "totalRuns"),
    "feedbackCoverageRate": ("outcomeReportedRuns", "succeededRuns"),
    "mismatchRate": ("mismatchRuns", "conclusiveOutcomeRuns"),
}
DURATIONS = ("averageDurationMs", "p95DurationMs")


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _positive_integer(value, field):
    if type(value) is not int or value <= 0:
        raise ValueError(f"{field} must be a positive integer")
    return value


def _read_jsonl(path):
    snapshots = {}
    try:
        source = Path(path).open(encoding="utf-8")
    except OSError as error:
        raise ValueError(f"could not read snapshot: {error}") from error
    with source:
        for line_number, line in enumerate(source, 1):
            if not line.strip():
                continue
            try:
                snapshot = json.loads(line, object_pairs_hook=_unique_object,
                                      parse_constant=lambda value: (_ for _ in ()).throw(
                                          ValueError(f"invalid JSON number: {value}")))
                normalized = _validate_snapshot(snapshot)
            except (json.JSONDecodeError, ValueError) as error:
                raise ValueError(f"invalid snapshot at line {line_number}: {error}") from error
            identity = (normalized["projectId"], normalized["versionId"])
            if identity in snapshots:
                raise ValueError(
                    f"duplicate project/version snapshot at line {line_number}: "
                    f"{identity[0]}/{identity[1]}"
                )
            snapshots[identity] = normalized
    return snapshots


def _validate_snapshot(snapshot):
    if not isinstance(snapshot, dict):
        raise ValueError("snapshot must be a JSON object")
    project_id = _positive_integer(snapshot.get("projectId"), "projectId")
    version_id = _positive_integer(snapshot.get("versionId"), "versionId")
    version_number = _positive_integer(snapshot.get("versionNumber"), "versionNumber")
    window_days = _positive_integer(snapshot.get("windowDays"), "windowDays")
    for field in ("windowStart", "generatedAt", "collectedAt"):
        if not isinstance(snapshot.get(field), str) or not snapshot[field].strip():
            raise ValueError(f"{field} must be a non-empty string")

    summary = snapshot.get("summary")
    if not isinstance(summary, dict):
        raise ValueError("summary must be a JSON object")
    safe_summary = {}
    for field in COUNTS:
        value = summary.get(field)
        if type(value) is not int or value < 0:
            raise ValueError(f"summary.{field} must be a non-negative integer")
        safe_summary[field] = value
    for field in RATES:
        value = summary.get(field)
        if (not isinstance(value, (int, float)) or isinstance(value, bool)
                or not math.isfinite(value) or not 0 <= value <= 1):
            raise ValueError(f"summary.{field} must be a finite rate between 0 and 1")
        safe_summary[field] = value
    for field in DURATIONS:
        value = summary.get(field)
        if (not isinstance(value, (int, float)) or isinstance(value, bool)
                or not math.isfinite(value) or value < 0):
            raise ValueError(f"summary.{field} must be a non-negative finite number")
        safe_summary[field] = value

    if safe_summary["succeededRuns"] + safe_summary["failedRuns"] > safe_summary["totalRuns"]:
        raise ValueError("summary succeededRuns + failedRuns exceeds totalRuns")
    if safe_summary["outcomeReportedRuns"] > safe_summary["totalRuns"]:
        raise ValueError("summary.outcomeReportedRuns exceeds totalRuns")
    if safe_summary["conclusiveOutcomeRuns"] > safe_summary["outcomeReportedRuns"]:
        raise ValueError("summary.conclusiveOutcomeRuns exceeds outcomeReportedRuns")
    for field, (numerator, denominator) in RATES.items():
        expected = safe_summary[numerator] / safe_summary[denominator] if safe_summary[denominator] else 0.0
        if not math.isclose(safe_summary[field], expected, rel_tol=0.0, abs_tol=1e-12):
            raise ValueError(f"summary.{field} does not match its count denominator")

    sample_counts = snapshot.get("sampleCounts")
    if not isinstance(sample_counts, dict):
        raise ValueError("sampleCounts must be a JSON object")
    safe_samples = {}
    expected_samples = {
        "failureRate": safe_summary["totalRuns"],
        "feedbackCoverageRate": safe_summary["succeededRuns"],
        "mismatchRate": safe_summary["conclusiveOutcomeRuns"],
        "averageDurationMs": safe_summary["totalRuns"],
        "p95DurationMs": safe_summary["totalRuns"],
    }
    for field, expected in expected_samples.items():
        value = sample_counts.get(field)
        if type(value) is not int or value < 0:
            raise ValueError(f"sampleCounts.{field} must be a non-negative integer")
        if value != expected:
            raise ValueError(f"sampleCounts.{field} does not match its metric denominator")
        safe_samples[field] = value

    return {
        "projectId": project_id,
        "versionId": version_id,
        "versionNumber": version_number,
        "windowDays": window_days,
        "windowStart": snapshot["windowStart"],
        "generatedAt": snapshot["generatedAt"],
        "collectedAt": snapshot["collectedAt"],
        "summary": safe_summary,
        "sampleCounts": safe_samples,
    }


def _metric(before, after, field, numerator=None, denominator=None):
    old_value = before["summary"][field]
    new_value = after["summary"][field]
    old_denominator = before["sampleCounts"][field] if field in before["sampleCounts"] else None
    new_denominator = after["sampleCounts"][field] if field in after["sampleCounts"] else None
    if denominator:
        old_denominator = before["summary"][denominator]
        new_denominator = after["summary"][denominator]
        old_numerator = before["summary"][numerator]
        new_numerator = after["summary"][numerator]
    else:
        old_numerator = new_numerator = None
    same_window = before["windowDays"] == after["windowDays"]
    return {
        "before": old_value,
        "after": new_value,
        "delta": new_value - old_value if same_window else None,
        "beforeNumerator": old_numerator,
        "afterNumerator": new_numerator,
        "beforeDenominator": old_denominator,
        "afterDenominator": new_denominator,
        "comparable": same_window,
    }


def compare_snapshots(before_rows, after_rows):
    identities = sorted(set(before_rows) | set(after_rows))
    results = []
    for project_id, version_id in identities:
        before = before_rows.get((project_id, version_id))
        after = after_rows.get((project_id, version_id))
        if before is None or after is None:
            results.append({
                "projectId": project_id,
                "versionId": version_id,
                "status": "missing_before" if before is None else "missing_after",
            })
            continue
        window_matches = before["windowDays"] == after["windowDays"]
        metrics = {}
        for field, (numerator, denominator) in RATES.items():
            metrics[field] = _metric(before, after, field, numerator, denominator)
        for field in DURATIONS:
            metrics[field] = _metric(before, after, field)
        counts = {
            field: {
                "before": before["summary"][field],
                "after": after["summary"][field],
                "delta": after["summary"][field] - before["summary"][field]
                if window_matches else None,
            }
            for field in COUNTS
        }
        results.append({
            "projectId": project_id,
            "versionId": version_id,
            "versionNumber": after["versionNumber"],
            "status": "comparable" if window_matches else "incomparable_window",
            "windowDays": {"before": before["windowDays"], "after": after["windowDays"]},
            "windowStart": {"before": before["windowStart"], "after": after["windowStart"]},
            "counts": counts,
            "metrics": metrics,
        })
    return {"schemaVersion": 1, "projects": results}


def _write_report(report, output_path):
    contents = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if output_path == "-":
        sys.stdout.write(contents)
        return
    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent, prefix=f".{output.name}.",
            delete=False,
        ) as target:
            temporary_path = target.name
            os.chmod(temporary_path, 0o600)
            target.write(contents)
        os.replace(temporary_path, output)
    except OSError:
        if temporary_path and os.path.exists(temporary_path):
            os.unlink(temporary_path)
        raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", required=True, help="earlier JSONL snapshot")
    parser.add_argument("--after", required=True, help="later JSONL snapshot")
    parser.add_argument("--output", default="-", help="JSON report path, or - for stdout")
    args = parser.parse_args(argv)
    try:
        report = compare_snapshots(_read_jsonl(args.before), _read_jsonl(args.after))
        _write_report(report, args.output)
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
