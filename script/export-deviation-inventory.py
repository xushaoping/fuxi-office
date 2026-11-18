#!/usr/bin/env python3
"""Export all observed deviation cases, including unreviewed cases, without business text."""

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
MAX_RECORDS = 100_000
MAX_RESPONSE_BYTES = 4 * 1024 * 1024
MAX_I64 = 9_223_372_036_854_775_807


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def reject_constant(_):
    raise ValueError("non-finite JSON number")


def positive_id(value):
    return type(value) is int and 0 < value <= MAX_I64


def fetch_inventory(api_base, token, project_id):
    if not positive_id(project_id):
        raise ValueError("project ID must be a positive signed 64-bit integer")
    parsed = urllib.parse.urlsplit(api_base)
    if (parsed.scheme not in {"http", "https"} or not parsed.hostname
            or parsed.username is not None or parsed.password is not None
            or parsed.query or parsed.fragment):
        raise ValueError("API base must be an HTTP(S) URL without credentials, query or fragment")
    if not isinstance(token, str) or not token.strip() or "\n" in token or "\r" in token:
        raise ValueError("API token must be non-empty and contain no line breaks")
    opener = urllib.request.build_opener(NoRedirect())

    def traversal(through_id=None):
        after_id = 0
        rows, seen_runs = [], set()
        while True:
            query = {"afterId": after_id, "limit": PAGE_SIZE}
            if through_id is not None:
                query["throughId"] = through_id
            request = urllib.request.Request(
                f"{api_base.rstrip('/')}/api/projects/{project_id}/deviation-analysis-inventory?"
                + urllib.parse.urlencode(query),
                headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
            )
            try:
                with opener.open(request, timeout=30) as response:
                    body = response.read(MAX_RESPONSE_BYTES + 1)
            except urllib.error.HTTPError as error:
                raise ValueError(f"inventory API request failed with HTTP {error.code}") from None
            except (urllib.error.URLError, TimeoutError, OSError):
                raise ValueError("inventory API request failed") from None
            if len(body) > MAX_RESPONSE_BYTES:
                raise ValueError("inventory API response exceeds size limit")
            try:
                page = json.loads(body, object_pairs_hook=unique_object, parse_constant=reject_constant)
            except (ValueError, UnicodeDecodeError, RecursionError):
                raise ValueError("inventory API returned invalid JSON") from None
            if not isinstance(page, dict):
                raise ValueError("inventory API page must be an object")
            boundary = page.get("throughId")
            if type(boundary) is not int or not 0 <= boundary <= MAX_I64:
                raise ValueError("inventory API returned an invalid boundary")
            if through_id is not None and boundary != through_id:
                raise ValueError("inventory API changed its boundary")
            through_id = boundary
            records = page.get("records")
            if not isinstance(records, list) or len(records) > PAGE_SIZE:
                raise ValueError("inventory API returned an invalid records page")
            for record in records:
                if not isinstance(record, dict):
                    raise ValueError("inventory API returned a non-object record")
                record_id = record.get("id")
                run_id = record.get("decisionRunId")
                if not positive_id(record_id) or not after_id < record_id <= through_id:
                    raise ValueError("inventory IDs must advance within the frozen boundary")
                if (not positive_id(record.get("projectId"))
                        or record["projectId"] != project_id):
                    raise ValueError("inventory record belongs to a different project")
                if not positive_id(run_id) or run_id in seen_runs:
                    raise ValueError("inventory decision run IDs must be positive and unique")
                status = record.get("status")
                if not isinstance(status, str) or status not in {"candidate", "insufficient_evidence"}:
                    raise ValueError("inventory attribution status is unsupported")
                confidence = record.get("confidence")
                if (type(confidence) not in {int, float} or not 0 <= confidence <= 1
                        or not math.isfinite(confidence)):
                    raise ValueError("inventory confidence must be finite and between 0 and 1")
                after_id = record_id
                seen_runs.add(run_id)
                rows.append({"caseId": f"deviation-{record_id}", "decisionRunId": run_id,
                             "status": status, "confidence": confidence})
                if len(rows) > MAX_RECORDS:
                    raise ValueError(f"inventory exceeds {MAX_RECORDS} cases")
            if len(records) < PAGE_SIZE:
                return through_id, rows

    through_id, rows = traversal()
    _, rechecked = traversal(through_id)
    if rows != rechecked:
        raise ValueError("deviation inventory changed during export; reload and try again")
    return rows


def write_private_export(path, api_base, token, project_id):
    rows = fetch_inventory(api_base, token, project_id)
    destination = Path(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=destination.parent,
                                         prefix=f".{destination.name}.", delete=False) as output:
            temporary = output.name
            os.chmod(temporary, 0o600)
            for row in rows:
                output.write(json.dumps(row, ensure_ascii=False, allow_nan=False) + "\n")
        os.replace(temporary, destination)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)
    return len(rows)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project_id", type=int)
    parser.add_argument("--api-base", default=os.environ.get("API_BASE", "http://127.0.0.1:3000"))
    parser.add_argument("--token", default=os.environ.get("API_TOKEN"))
    parser.add_argument("--output", required=True, help="private JSONL inventory destination")
    args = parser.parse_args(argv)
    try:
        count = write_private_export(args.output, args.api_base, args.token, args.project_id)
    except (ValueError, OSError) as error:
        parser.error(str(error))
    print(f"Exported {count} deviation cases", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
