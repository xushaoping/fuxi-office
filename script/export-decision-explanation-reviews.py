#!/usr/bin/env python3
"""Export project explanation review labels without explanation content."""

import argparse
import json
import os
from pathlib import Path
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

PAGE_SIZE = 500
MAX_RECORDS = 100_000
LABELS = {"readable", "needs_revision", "inconclusive"}


def export_reviews(api_base, token, project_id, output):
    after_id = 0
    exported = 0
    seen_pairs = set()
    headers = {"Authorization": f"Bearer {token}", "Accept": "application/json"}
    while True:
        query = urllib.parse.urlencode({"afterId": after_id, "limit": PAGE_SIZE})
        url = f"{api_base.rstrip('/')}/api/projects/{project_id}/decision-explanation-reviews?{query}"
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                records = json.load(response)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as error:
            raise RuntimeError(f"failed to read explanation reviews after ID {after_id}: {error}") from error
        if not isinstance(records, list):
            raise RuntimeError("explanation review API returned a non-list response")
        if not records:
            break
        for record in records:
            if not isinstance(record, dict):
                raise RuntimeError("explanation review API returned a non-object record")
            record_id = record.get("id")
            explanation_id = record.get("explanationId")
            explanation_revision = record.get("explanationRevision")
            reviewer = record.get("reviewerUsername")
            label = record.get("reviewLabel")
            if type(record_id) is not int or record_id <= after_id:
                raise RuntimeError("explanation review API returned an invalid or unordered ID")
            if type(explanation_id) is not int or explanation_id <= 0:
                raise RuntimeError(f"explanation review {record_id} has an invalid explanation ID")
            if type(explanation_revision) is not int or not 0 < explanation_revision <= 9_223_372_036_854_775_807:
                raise RuntimeError(f"explanation review {record_id} has an invalid explanation revision")
            if not isinstance(reviewer, str) or not reviewer or len(reviewer) > 255:
                raise RuntimeError(f"explanation review {record_id} has an invalid reviewer")
            if not isinstance(label, str) or label not in LABELS:
                raise RuntimeError(f"explanation review {record_id} has an unsupported label")
            pair = (explanation_id, explanation_revision, reviewer)
            if pair in seen_pairs:
                raise RuntimeError("explanation review API returned a duplicate reviewer/explanation pair")
            seen_pairs.add(pair)
            after_id = record_id
            output.write(json.dumps({
                "explanationId": explanation_id,
                "explanationRevision": explanation_revision,
                "reviewer": reviewer,
                "label": label,
            }, ensure_ascii=False) + "\n")
            exported += 1
            if exported > MAX_RECORDS:
                raise RuntimeError(f"export exceeds {MAX_RECORDS} review records")
        if len(records) < PAGE_SIZE:
            break
    return exported


def write_private_export(path, api_base, token, project_id):
    output_path = Path(path)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output_path.parent,
            prefix=f".{output_path.name}.", delete=False,
        ) as target:
            temporary_path = target.name
            os.chmod(temporary_path, 0o600)
            count = export_reviews(api_base, token, project_id, target)
        os.replace(temporary_path, output_path)
        os.chmod(output_path, 0o600)
        return count
    except OSError:
        if temporary_path and os.path.exists(temporary_path):
            os.unlink(temporary_path)
        raise
    except Exception:
        if temporary_path and os.path.exists(temporary_path):
            os.unlink(temporary_path)
        raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project_id", type=int)
    parser.add_argument("--api-base", default=os.environ.get("API_BASE", "http://127.0.0.1:3000"))
    parser.add_argument("--token", default=os.environ.get("API_TOKEN"), required="API_TOKEN" not in os.environ)
    parser.add_argument("--output", required=True, help="private JSONL output path")
    args = parser.parse_args(argv)
    if args.project_id <= 0:
        parser.error("project_id must be positive")
    try:
        count = write_private_export(args.output, args.api_base, args.token, args.project_id)
    except (OSError, RuntimeError) as error:
        parser.error(str(error))
    print(f"Exported {count} decision explanation reviews", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
