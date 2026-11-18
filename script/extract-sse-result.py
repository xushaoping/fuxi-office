#!/usr/bin/env python3
"""Extract exactly one terminal JSON event from a captured SSE response."""

import json
from pathlib import Path
import sys


def extract_result(path, result_event):
    matches = []
    event_name = "message"
    data_lines = []

    def finish_event():
        if event_name == "error":
            try:
                message = json.loads("\n".join(data_lines)).get("error", "workflow stream failed")
            except (json.JSONDecodeError, AttributeError):
                message = "workflow stream returned an invalid error event"
            raise ValueError(message)
        if event_name == result_event:
            try:
                payload = json.loads("\n".join(data_lines))
            except json.JSONDecodeError as error:
                raise ValueError(f"terminal {result_event} event is invalid JSON") from error
            if not isinstance(payload, dict):
                raise ValueError(f"terminal {result_event} event must contain a JSON object")
            matches.append(payload)

    for line in Path(path).read_text(encoding="utf-8").splitlines() + [""]:
        if not line:
            finish_event()
            event_name = "message"
            data_lines = []
        elif line.startswith(":"):
            continue
        elif line.startswith("event:"):
            event_name = line[6:].strip() or "message"
        elif line.startswith("data:"):
            value = line[5:]
            data_lines.append(value[1:] if value.startswith(" ") else value)

    if len(matches) != 1:
        raise ValueError(f"expected exactly one terminal {result_event} event, found {len(matches)}")
    return matches[0]


def main(argv):
    if len(argv) != 3:
        raise SystemExit("usage: extract-sse-result.py SSE_FILE EVENT_NAME")
    try:
        payload = extract_result(argv[1], argv[2])
    except (OSError, UnicodeError, ValueError) as error:
        raise SystemExit(f"error: {error}") from error
    print(json.dumps(payload, ensure_ascii=False, separators=(",", ":")))


if __name__ == "__main__":
    main(sys.argv)
