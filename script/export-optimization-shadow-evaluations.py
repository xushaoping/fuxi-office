#!/usr/bin/env python3
"""Export privacy-limited optimization shadow qualification summaries."""

import argparse
import json
import math
import os
from pathlib import Path
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

PAGE_SIZE = 500
MAX_RESPONSE_BYTES = 1_048_576
MAX_RECORDS = 100_000
MAX_I64 = 9_223_372_036_854_775_807
OUTCOMES = {
    "blocked_project_risk",
    "blocked_finding_code",
    "blocked_unlabeled_samples",
    "blocked_insufficient_samples",
    "blocked_simulation",
    "blocked_change_risk",
    "eligible_shadow_candidate",
}
FINDINGS = {"HIGH_FAILURE_RATE", "HIGH_MISMATCH_RATE"}
SAMPLE_SOURCES = {"approved_business_tests", "successful_history_runs"}


class RejectRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


def open_url(request, timeout=30):
    return urllib.request.build_opener(RejectRedirects()).open(request, timeout=timeout)


def _positive_int(value, field, record_id):
    if type(value) is not int or value <= 0 or value > MAX_I64:
        raise ValueError(f"shadow evaluation {record_id} has an invalid {field}")
    return value


def validate_evaluation(row, project_id, after_id):
    if not isinstance(row, dict):
        raise ValueError("shadow evaluation item is not an object")
    evaluation_id = _positive_int(row.get("id"), "ID", "unknown")
    if evaluation_id <= after_id:
        raise ValueError("shadow evaluation API returned an invalid or unordered ID")
    if row.get("projectId") != project_id or type(row.get("projectId")) is not int:
        raise ValueError(f"shadow evaluation {evaluation_id} belongs to an unexpected project")

    for field in (
        "simulationId",
        "proposalId",
        "sourceVersionId",
        "candidateVersionId",
    ):
        _positive_int(row.get(field), field, evaluation_id)
    outcome = row.get("outcome")
    if not isinstance(outcome, str) or outcome not in OUTCOMES:
        raise ValueError(f"shadow evaluation {evaluation_id} has an unsupported outcome")
    finding = row.get("findingCode")
    if not isinstance(finding, str) or finding not in FINDINGS:
        raise ValueError(f"shadow evaluation {evaluation_id} has an unsupported finding code")
    sample_source = row.get("sampleSource")
    if not isinstance(sample_source, str) or sample_source not in SAMPLE_SOURCES:
        raise ValueError(f"shadow evaluation {evaluation_id} has an unsupported sample source")
    test_set_size = row.get("testSetSize")
    if type(test_set_size) is not int or not 0 <= test_set_size <= 1000:
        raise ValueError(f"shadow evaluation {evaluation_id} has an invalid test set size")

    accuracies = {}
    for field in ("baselineAccuracy", "simulatedAccuracy"):
        value = row.get(field)
        if (not isinstance(value, (int, float)) or isinstance(value, bool)
                or not math.isfinite(value) or not 0 <= value <= 100):
            raise ValueError(f"shadow evaluation {evaluation_id} has an invalid {field}")
        accuracies[field] = float(value)
    created_at = row.get("createdAt")
    if not isinstance(created_at, str) or not created_at or len(created_at) > 64:
        raise ValueError(f"shadow evaluation {evaluation_id} has an invalid createdAt")

    return {
        "id": evaluation_id,
        "simulationId": row["simulationId"],
        "projectId": project_id,
        "proposalId": row["proposalId"],
        "sourceVersionId": row["sourceVersionId"],
        "candidateVersionId": row["candidateVersionId"],
        "outcome": outcome,
        "findingCode": finding,
        "sampleSource": sample_source,
        "testSetSize": test_set_size,
        **accuracies,
        "createdAt": created_at,
    }


def fetch_page(api_base, token, project_id, after_id):
    query = urllib.parse.urlencode({"afterId": after_id, "limit": PAGE_SIZE})
    url = f"{api_base.rstrip('/')}/api/projects/{project_id}/optimization-shadow-evaluations?{query}"
    request = urllib.request.Request(
        url,
        headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
    )
    try:
        with open_url(request) as response:
            raw = response.read(MAX_RESPONSE_BYTES + 1)
    except (urllib.error.URLError, TimeoutError) as error:
        raise RuntimeError(f"failed to read shadow evaluations after ID {after_id}: {error}") from error
    if len(raw) > MAX_RESPONSE_BYTES:
        raise RuntimeError("shadow evaluation response exceeds the 1 MiB limit")
    try:
        payload = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("shadow evaluation response is not valid JSON") from error
    if not isinstance(payload, list):
        raise RuntimeError("shadow evaluation API returned a non-list response")
    if len(payload) > PAGE_SIZE:
        raise RuntimeError("shadow evaluation API returned more than the requested page size")
    return payload


def export_evaluations(api_base, token, project_id, output):
    if not token or not token.strip():
        raise ValueError("API_TOKEN is required")
    if type(project_id) is not int or project_id <= 0 or project_id > MAX_I64:
        raise ValueError("project ID must be a positive integer")
    after_id = 0
    exported = 0
    while True:
        page = fetch_page(api_base, token, project_id, after_id)
        if not page:
            break
        for row in page:
            record = validate_evaluation(row, project_id, after_id)
            after_id = record["id"]
            output.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n")
            exported += 1
            if exported > MAX_RECORDS:
                raise ValueError(f"export exceeds the {MAX_RECORDS} record limit")
        if len(page) < PAGE_SIZE:
            break
    return exported


def write_export(api_base, token, project_id, output_path):
    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent,
            prefix=f".{output.name}.", delete=False,
        ) as target:
            temporary_path = target.name
            os.chmod(temporary_path, 0o600)
            count = export_evaluations(api_base, token, project_id, target)
        os.replace(temporary_path, output)
        return count
    except (OSError, RuntimeError, ValueError):
        if temporary_path and os.path.exists(temporary_path):
            os.unlink(temporary_path)
        raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project_id", type=int)
    parser.add_argument("--api-base", default=os.environ.get("API_BASE", "http://127.0.0.1:3000"))
    parser.add_argument("--output", required=True, help="private JSONL output path")
    args = parser.parse_args(argv)
    if args.project_id <= 0:
        parser.error("project_id must be positive")
    try:
        count = write_export(
            args.api_base, os.environ.get("API_TOKEN", ""), args.project_id, args.output
        )
    except (OSError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"exported {count} shadow evaluations: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
