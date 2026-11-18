#!/usr/bin/env python3
"""Export reviewed deviation attribution metadata as privacy-limited JSONL."""

import argparse
import json
import math
import os
import sys
import urllib.error
import urllib.parse
import urllib.request


def export_reviews(api_base, token, project_id, output):
    after_id = 0
    exported = 0
    headers = {"Authorization": f"Bearer {token}", "Accept": "application/json"}
    while True:
        query = urllib.parse.urlencode({"afterId": after_id, "limit": 500})
        url = f"{api_base.rstrip('/')}/api/projects/{project_id}/deviation-analysis-reviews?{query}"
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                records = json.load(response)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as error:
            raise RuntimeError(f"failed to read deviation reviews after ID {after_id}: {error}") from error
        if not isinstance(records, list):
            raise RuntimeError("deviation review API returned a non-list response")
        if not records:
            break
        for record in records:
            record_id = record.get("id")
            label = record.get("reviewLabel")
            run_id = record.get("decisionRunId")
            status = record.get("status")
            if type(record_id) is not int or record_id <= after_id:
                raise RuntimeError("deviation review API returned an invalid or unordered ID")
            after_id = record_id
            if label not in ("correct", "incorrect", "inconclusive"):
                raise RuntimeError(f"deviation review {record_id} has an unsupported label")
            if type(run_id) is not int or run_id <= 0:
                raise RuntimeError(f"deviation review {record_id} has an invalid run ID")
            if status not in ("candidate", "insufficient_evidence"):
                raise RuntimeError(f"deviation review {record_id} has an unsupported status")
            if status == "insufficient_evidence" and label != "inconclusive":
                raise RuntimeError(f"deviation review {record_id} has a label inconsistent with its status")
            confidence = record.get("confidence")
            if (not isinstance(confidence, (int, float)) or isinstance(confidence, bool)
                    or not math.isfinite(confidence) or not 0 <= confidence <= 1):
                raise RuntimeError(f"deviation review {record_id} has invalid confidence")
            attributed_rule = record.get("attributedRule")
            if attributed_rule is not None and not isinstance(attributed_rule, str):
                raise RuntimeError(f"deviation review {record_id} has an invalid attributed rule")
            output.write(json.dumps({
                "caseId": f"deviation-{record_id}",
                "decisionRunId": run_id,
                "status": status,
                "attributedRule": attributed_rule,
                "confidence": confidence,
                "label": label,
            }, ensure_ascii=False) + "\n")
            exported += 1
        if len(records) < 500:
            break
    return exported


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project_id", type=int)
    parser.add_argument("--api-base", default=os.environ.get("API_BASE", "http://127.0.0.1:3000"))
    parser.add_argument("--token", default=os.environ.get("API_TOKEN"), required="API_TOKEN" not in os.environ)
    parser.add_argument("--output", help="write JSONL to this path instead of stdout")
    args = parser.parse_args()
    if args.project_id <= 0:
        parser.error("project_id must be positive")
    try:
        if args.output:
            with open(args.output, "w", encoding="utf-8") as target:
                count = export_reviews(args.api_base, args.token, args.project_id, target)
        else:
            count = export_reviews(args.api_base, args.token, args.project_id, sys.stdout)
    except (OSError, RuntimeError) as error:
        parser.error(str(error))
    print(f"Exported {count} reviewed deviation analyses", file=sys.stderr)


if __name__ == "__main__":
    main()
