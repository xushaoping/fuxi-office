"""Export reviewed drift events or scan windows as JSONL replay cases."""

import argparse
import json
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
        url = f"{api_base.rstrip('/')}/api/projects/{project_id}/drift-event-reviews?{query}"
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                events = json.load(response)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as error:
            raise RuntimeError(f"failed to read drift reviews after event {after_id}: {error}") from error
        if not isinstance(events, list):
            raise RuntimeError("drift review API returned a non-list response")
        if not events:
            break
        for event in events:
            event_id = event.get("id")
            label = event.get("reviewLabel")
            evidence = event.get("evidence")
            if not isinstance(event_id, int) or event_id <= after_id:
                raise RuntimeError("drift review API returned an invalid or unordered event ID")
            after_id = event_id
            if label == "inconclusive":
                continue
            if label not in ("confirmed_drift", "false_positive"):
                raise RuntimeError(f"event {event_id} has an unsupported review label")
            if not isinstance(evidence, dict) or not isinstance(evidence.get("summary"), dict):
                raise RuntimeError(f"event {event_id} is missing its analytics evidence")
            record = {
                "caseId": f"event-{event_id}",
                "label": "drift" if label == "confirmed_drift" else "normal",
                "report": {
                    "summary": evidence["summary"],
                    "findings": evidence.get("findings", []),
                },
            }
            output.write(json.dumps(record, ensure_ascii=False) + "\n")
            exported += 1
        if len(events) < 500:
            break
    return exported


def export_windows(api_base, token, project_id, output):
    after_id = 0
    exported = 0
    headers = {"Authorization": f"Bearer {token}", "Accept": "application/json"}
    while True:
        query = urllib.parse.urlencode({"afterId": after_id, "limit": 500})
        url = f"{api_base.rstrip('/')}/api/projects/{project_id}/drift-scan-windows?{query}"
        request = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                windows = json.load(response)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as error:
            raise RuntimeError(f"failed to read drift scan windows after ID {after_id}: {error}") from error
        if not isinstance(windows, list):
            raise RuntimeError("drift scan window API returned a non-list response")
        if not windows:
            break
        for window in windows:
            window_id = window.get("id")
            label = window.get("reviewLabel")
            evidence = window.get("evidence")
            if not isinstance(window_id, int) or window_id <= after_id:
                raise RuntimeError("drift scan window API returned an invalid or unordered ID")
            after_id = window_id
            if label == "inconclusive":
                continue
            if label not in ("drift", "normal"):
                continue
            if not isinstance(evidence, dict) or not isinstance(evidence.get("summary"), dict):
                raise RuntimeError(f"window {window_id} is missing its analytics evidence")
            scan_day = window.get("scanDay", "unknown-day")
            output.write(json.dumps({
                "caseId": f"window-{window_id}-{project_id}-{window.get('sourceVersionId')}-{scan_day}",
                "label": label,
                "projectId": project_id,
                "sourceVersionId": window.get("sourceVersionId"),
                "scanDay": scan_day,
                "report": evidence,
            }, ensure_ascii=False) + "\n")
            exported += 1
        if len(windows) < 500:
            break
    return exported


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project_id", type=int)
    parser.add_argument("--api-base", default=os.environ.get("API_BASE", "http://127.0.0.1:3000"))
    parser.add_argument("--token", default=os.environ.get("API_TOKEN"), required="API_TOKEN" not in os.environ)
    parser.add_argument("--output", help="write JSONL to this path instead of stdout")
    parser.add_argument("--kind", choices=("events", "windows"), default="events",
                        help="export triggered events for precision audits or reviewed daily windows for full replay")
    args = parser.parse_args()
    if args.project_id <= 0:
        parser.error("project_id must be positive")
    try:
        if args.output:
            with open(args.output, "w", encoding="utf-8") as target:
                exporter = export_windows if args.kind == "windows" else export_reviews
                count = exporter(args.api_base, args.token, args.project_id, target)
        else:
            exporter = export_windows if args.kind == "windows" else export_reviews
            count = exporter(args.api_base, args.token, args.project_id, sys.stdout)
    except (OSError, RuntimeError) as error:
        parser.error(str(error))
    print(f"Exported {count} reviewed drift {args.kind}", file=sys.stderr)


if __name__ == "__main__":
    main()
