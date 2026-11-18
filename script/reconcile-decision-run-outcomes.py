#!/usr/bin/env python3
"""Reconcile decision run outcome states using a scoped decision service key."""

import argparse
import json
import os
from pathlib import Path
import sys
import tempfile
from urllib.error import HTTPError, URLError
from urllib.request import HTTPRedirectHandler, Request, build_opener

MAX_INPUT_BYTES = 1_048_576
MAX_RESPONSE_BYTES = 1_048_576
MAX_RUN_IDS = 10_000
BATCH_SIZE = 100
MAX_I64 = 9_223_372_036_854_775_807
STATES = {"ready", "already_reported", "not_succeeded", "unavailable"}


class RejectRedirects(HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


def open_url(request, timeout):
    return build_opener(RejectRedirects()).open(request, timeout=timeout)


def read_run_ids(input_path):
    source = Path(input_path)
    if source.stat().st_size > MAX_INPUT_BYTES:
        raise ValueError("input exceeds the 1 MiB limit")
    run_ids = []
    seen = set()
    with source.open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            if not line.strip():
                continue
            if len(line.encode("utf-8")) > 8192:
                raise ValueError(f"line {line_number} exceeds the 8 KiB limit")
            try:
                row = json.loads(line)
            except json.JSONDecodeError as error:
                raise ValueError(f"line {line_number} is not valid JSON") from error
            if not isinstance(row, dict) or set(row) != {"runId"}:
                raise ValueError(f"line {line_number} must contain only runId")
            run_id = row["runId"]
            if type(run_id) is not int or run_id <= 0 or run_id > MAX_I64:
                raise ValueError(f"line {line_number} runId must be a positive signed 64-bit integer")
            if run_id in seen:
                raise ValueError(f"line {line_number} duplicates runId {run_id}")
            seen.add(run_id)
            run_ids.append(run_id)
            if len(run_ids) > MAX_RUN_IDS:
                raise ValueError(f"input exceeds the {MAX_RUN_IDS} run ID limit")
    if not run_ids:
        raise ValueError("input must contain at least one run ID")
    return run_ids


def validate_response(payload, expected_ids):
    if not isinstance(payload, dict) or not isinstance(payload.get("results"), list):
        raise ValueError("reconciliation response must contain a results array")
    records = payload["results"]
    if len(records) != len(expected_ids):
        raise ValueError("reconciliation response count does not match the request")
    validated = []
    for index, (record, expected_id) in enumerate(zip(records, expected_ids)):
        if not isinstance(record, dict):
            raise ValueError(f"reconciliation response item {index} is not an object")
        if type(record.get("runId")) is not int or record["runId"] != expected_id:
            raise ValueError(f"reconciliation response item {index} is out of request order")
        state = record.get("state")
        if not isinstance(state, str) or state not in STATES:
            raise ValueError(f"reconciliation response item {index} has an unsupported state")
        validated.append({"runId": expected_id, "state": state})
    return validated


def reconcile_batch(api_base, decision_key, project_id, service_id, run_ids):
    body = json.dumps({"serviceId": service_id, "runIds": run_ids}, separators=(",", ":")).encode()
    request = Request(
        f"{api_base.rstrip('/')}/api/public/projects/{project_id}/decision-runs/outcomes:reconcile",
        data=body,
        method="POST",
        headers={
            "X-Decision-Key": decision_key,
            "Content-Type": "application/json",
            "Accept": "application/json",
        },
    )
    with open_url(request, timeout=30) as response:
        raw = response.read(MAX_RESPONSE_BYTES + 1)
    if len(raw) > MAX_RESPONSE_BYTES:
        raise ValueError("reconciliation response exceeds the 1 MiB limit")
    try:
        payload = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError("reconciliation response is not valid JSON") from error
    return validate_response(payload, run_ids)


def reconcile(api_base, decision_key, project_id, service_id, run_ids):
    if not decision_key.strip():
        raise ValueError("DECISION_KEY is required")
    if type(project_id) is not int or project_id <= 0 or project_id > MAX_I64:
        raise ValueError("project ID must be a positive signed 64-bit integer")
    if not isinstance(service_id, str) or not service_id.strip() or len(service_id) > 255:
        raise ValueError("service ID must contain 1 to 255 characters")
    if not run_ids or len(run_ids) > MAX_RUN_IDS:
        raise ValueError(f"run ID count must be between 1 and {MAX_RUN_IDS}")
    if any(type(run_id) is not int or run_id <= 0 or run_id > MAX_I64 for run_id in run_ids):
        raise ValueError("run IDs must be positive signed 64-bit integers")
    if len(set(run_ids)) != len(run_ids):
        raise ValueError("run IDs must be unique")

    rows = []
    for offset in range(0, len(run_ids), BATCH_SIZE):
        batch = run_ids[offset:offset + BATCH_SIZE]
        rows.extend(reconcile_batch(api_base, decision_key, project_id, service_id, batch))
    return rows


def write_reconciliation(api_base, decision_key, project_id, service_id, run_ids, output_path):
    rows = reconcile(api_base, decision_key, project_id, service_id, run_ids)
    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent, prefix=f".{output.name}.", delete=False
        ) as target:
            temporary_path = target.name
            os.chmod(temporary_path, 0o600)
            for row in rows:
                target.write(json.dumps(row, separators=(",", ":")) + "\n")
        os.replace(temporary_path, output)
    except OSError:
        if temporary_path and os.path.exists(temporary_path):
            os.unlink(temporary_path)
        raise
    return len(rows)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-id", required=True, type=int)
    parser.add_argument("--service-id", required=True)
    parser.add_argument("--input", required=True, help="JSONL with one {\"runId\": integer} object per line")
    parser.add_argument("--output", required=True, help="private JSONL output path")
    parser.add_argument("--api-base", default=os.environ.get("API_BASE", "http://127.0.0.1:3000"))
    args = parser.parse_args(argv)
    try:
        run_ids = read_run_ids(args.input)
        count = write_reconciliation(
            args.api_base,
            os.environ.get("DECISION_KEY", ""),
            args.project_id,
            args.service_id,
            run_ids,
            args.output,
        )
    except (HTTPError, URLError, OSError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"reconciled {count} decision runs: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
