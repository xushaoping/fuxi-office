#!/usr/bin/env python3
"""Export privacy-limited project analytics summaries as a JSONL baseline."""

import argparse
from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import re
import sys
import tempfile
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

MAX_RESPONSE_BYTES = 1_048_576
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
RATES = ("failureRate", "feedbackCoverageRate", "mismatchRate")


def _positive_integer(value, name):
    if type(value) is not int or value <= 0:
        raise ValueError(f"{name} must be a positive integer")
    return value


def normalize_report(report, requested_project_id, collected_at=None):
    if not isinstance(report, dict):
        raise ValueError("analytics response must be a JSON object")
    project_id = _positive_integer(report.get("projectId"), "projectId")
    version_id = _positive_integer(report.get("versionId"), "versionId")
    if project_id != requested_project_id:
        raise ValueError("analytics response projectId does not match the requested project")
    window_days = _positive_integer(report.get("windowDays"), "windowDays")
    summary = report.get("summary")
    if not isinstance(summary, dict):
        raise ValueError("analytics summary must be a JSON object")

    safe_summary = {}
    for key in COUNTS:
        value = summary.get(key)
        if type(value) is not int or value < 0:
            raise ValueError(f"summary.{key} must be a non-negative integer")
        safe_summary[key] = value
    for key in RATES:
        value = summary.get(key)
        if (not isinstance(value, (int, float)) or isinstance(value, bool)
                or not math.isfinite(value) or not 0 <= value <= 1):
            raise ValueError(f"summary.{key} must be a finite rate between 0 and 1")
        safe_summary[key] = value
    for key in ("averageDurationMs", "p95DurationMs"):
        value = summary.get(key)
        if (not isinstance(value, (int, float)) or isinstance(value, bool)
                or not math.isfinite(value) or value < 0):
            raise ValueError(f"summary.{key} must be a non-negative finite number")
        safe_summary[key] = value

    if safe_summary["succeededRuns"] + safe_summary["failedRuns"] > safe_summary["totalRuns"]:
        raise ValueError("summary succeededRuns + failedRuns exceeds totalRuns")
    if safe_summary["outcomeReportedRuns"] > safe_summary["totalRuns"]:
        raise ValueError("summary.outcomeReportedRuns exceeds totalRuns")
    if safe_summary["conclusiveOutcomeRuns"] > safe_summary["outcomeReportedRuns"]:
        raise ValueError("summary.conclusiveOutcomeRuns exceeds outcomeReportedRuns")

    expected_rates = {
        "failureRate": (safe_summary["failedRuns"], safe_summary["totalRuns"]),
        "feedbackCoverageRate": (
            safe_summary["outcomeReportedRuns"], safe_summary["succeededRuns"]
        ),
        "mismatchRate": (
            safe_summary["mismatchRuns"], safe_summary["conclusiveOutcomeRuns"]
        ),
    }
    for key, (numerator, denominator) in expected_rates.items():
        expected = numerator / denominator if denominator else 0.0
        if not math.isclose(safe_summary[key], expected, rel_tol=0.0, abs_tol=1e-12):
            raise ValueError(f"summary.{key} does not match its count denominator")

    timestamps = {}
    for key in ("windowStart", "generatedAt"):
        value = report.get(key)
        if not isinstance(value, str) or not value.strip():
            raise ValueError(f"{key} must be a non-empty timestamp string")
        timestamps[key] = value
    return {
        "projectId": project_id,
        "versionId": version_id,
        "versionNumber": _positive_integer(report.get("versionNumber"), "versionNumber"),
        "windowDays": window_days,
        "windowStart": timestamps["windowStart"],
        "generatedAt": timestamps["generatedAt"],
        "collectedAt": collected_at or datetime.now(timezone.utc).isoformat(),
        "summary": safe_summary,
        "sampleCounts": {
            "failureRate": safe_summary["totalRuns"],
            "feedbackCoverageRate": safe_summary["succeededRuns"],
            "mismatchRate": safe_summary["conclusiveOutcomeRuns"],
            "averageDurationMs": safe_summary["totalRuns"],
            "p95DurationMs": safe_summary["totalRuns"],
        },
    }


def parse_project_ids(value):
    parts = [item.strip() for item in value.split(",")]
    if not parts or any(not re.fullmatch(r"[1-9][0-9]*", item) for item in parts):
        raise ValueError("project IDs must be comma-separated positive integers")
    project_ids = [int(item) for item in parts]
    if any(project_id > 9_223_372_036_854_775_807 for project_id in project_ids):
        raise ValueError("project IDs must fit in a signed 64-bit integer")
    if len(set(project_ids)) != len(project_ids):
        raise ValueError("project IDs must be unique")
    return project_ids


def fetch_report(base_url, token, project_id):
    request = Request(
        f"{base_url.rstrip('/')}/api/projects/{project_id}/decision-analytics",
        headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
    )
    with urlopen(request, timeout=30) as response:
        body = response.read(MAX_RESPONSE_BYTES + 1)
    if len(body) > MAX_RESPONSE_BYTES:
        raise ValueError("analytics response exceeds the 1 MiB limit")
    try:
        return json.loads(body)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("analytics response is not valid JSON") from error


def export_baseline(base_url, token, project_ids, output_path):
    if not token.strip():
        raise ValueError("AUTH_TOKEN is required")
    if not project_ids:
        raise ValueError("at least one project ID is required")
    if len(set(project_ids)) != len(project_ids):
        raise ValueError("project IDs must be unique")

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
            for project_id in project_ids:
                report = fetch_report(base_url, token, project_id)
                snapshot = normalize_report(report, project_id)
                target.write(json.dumps(snapshot, ensure_ascii=False, separators=(",", ":")) + "\n")
        os.replace(temporary_path, output)
    except (HTTPError, URLError, OSError, ValueError) as error:
        if temporary_path and os.path.exists(temporary_path):
            os.unlink(temporary_path)
        raise RuntimeError(f"could not export decision analytics baseline: {error}") from error


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-ids", required=True, help="comma-separated project IDs")
    parser.add_argument("--base-url", default=os.environ.get("BASE_URL", "http://127.0.0.1:3000"))
    parser.add_argument("--output", required=True, help="JSONL output path")
    args = parser.parse_args(argv)
    try:
        project_ids = parse_project_ids(args.project_ids)
        export_baseline(args.base_url, os.environ.get("AUTH_TOKEN", ""), project_ids, args.output)
    except (RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"decision analytics baseline exported: {args.output} ({len(project_ids)} projects)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
