#!/usr/bin/env python3
"""Freeze all current Tester proposals as a private acceptance-review inventory."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

MAX_RESPONSE_BYTES = 4 * 1024 * 1024
MAX_PROPOSALS = 100_000
MAX_I64 = 9_223_372_036_854_775_807


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def reject_constant(_value):
    raise ValueError("non-finite JSON number")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def positive_id(value):
    return type(value) is int and 0 < value <= MAX_I64


def current_version(workspace, project_id):
    if not isinstance(workspace, dict):
        raise ValueError("workspace response must be an object")
    project, version = workspace.get("project"), workspace.get("version")
    if (not isinstance(project, dict) or not positive_id(project.get("id"))
            or project["id"] != project_id or not isinstance(version, dict)
            or not positive_id(version.get("id")) or not positive_id(version.get("projectId"))
            or version["projectId"] != project_id
            or type(project.get("currentDraftVersionId")) is not int
            or project["currentDraftVersionId"] != version["id"]):
        raise ValueError("project must have a matching current draft version")
    if version.get("status") not in {"draft", "validated", "reviewed"}:
        raise ValueError("current version is not an editable draft")
    return version["id"]


def business_snapshot(workflow, project_id):
    if (not isinstance(workflow, dict) or not positive_id(workflow.get("projectId"))
            or workflow["projectId"] != project_id
            or workflow.get("status") not in {"planned", "confirmed"}):
        raise ValueError("project has no planned Tester workflow")
    report = workflow.get("testerReport")
    if (not isinstance(report, dict) or type(report.get("schemaVersion")) is not int
            or report["schemaVersion"] != 1 or not isinstance(report.get("proposals"), list)
            or not 0 < len(report["proposals"]) <= MAX_PROPOSALS):
        raise ValueError("Tester report must contain a supported, non-empty proposal list")
    if (not isinstance(workflow.get("requirementText"), str)
            or not workflow["requirementText"].strip()
            or not isinstance(workflow.get("brief"), dict)
            or not isinstance(workflow.get("answers", []), list)
            or not isinstance(workflow.get("plan"), dict)):
        raise ValueError("Tester workflow is missing business context")
    proposals, seen = [], set()
    for proposal in report["proposals"]:
        if not isinstance(proposal, dict):
            raise ValueError("invalid Tester proposal")
        proposal_id = proposal.get("id")
        if (not isinstance(proposal_id, str)
                or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:-]{0,63}", proposal_id)
                or proposal_id in seen):
            raise ValueError("Tester proposal IDs must be valid and unique")
        seen.add(proposal_id)
        required_strings = ("category", "title", "rationale")
        if any(not isinstance(proposal.get(field), str) for field in required_strings):
            raise ValueError("Tester proposal business fields are invalid")
        fields = proposal.get("inputFields", [])
        if (not isinstance(fields, list) or any(not isinstance(field, str) for field in fields)
                or not isinstance(proposal.get("sourceReference", ""), str)
                or type(proposal.get("reviewRequired")) is not bool):
            raise ValueError("Tester proposal references are invalid")
        proposals.append({
            "id": proposal_id,
            **{field: proposal[field] for field in required_strings},
            "sourceReference": proposal.get("sourceReference", ""),
            "inputFields": sorted(fields),
            "expectedOutput": proposal.get("expectedOutput"),
            "reviewRequired": proposal["reviewRequired"],
        })
    return {
        "requirementText": workflow["requirementText"],
        "originalRequirementText": workflow.get("originalRequirementText"),
        "baseRevision": workflow.get("baseRevision"),
        "brief": workflow["brief"], "answers": workflow.get("answers", []),
        "plan": workflow["plan"],
        "testerReport": {"schemaVersion": 1, "proposals": sorted(proposals, key=lambda item: item["id"])},
    }


def inventory_rows(workspace, workflow, project_id):
    if not positive_id(project_id):
        raise ValueError("project ID must be a positive signed 64-bit integer")
    version_id = current_version(workspace, project_id)
    snapshot = business_snapshot(workflow, project_id)
    digest = hashlib.sha256(json.dumps(
        snapshot, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False,
    ).encode("utf-8")).hexdigest()
    return [{"projectId": project_id, "versionId": version_id, "reportDigest": digest,
             "proposalId": proposal["id"]} for proposal in snapshot["testerReport"]["proposals"]]


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

    def read(suffix):
        request = urllib.request.Request(
            f"{api_base.rstrip('/')}/api/projects/{project_id}{suffix}",
            headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
        )
        try:
            with opener.open(request, timeout=30) as response:
                body = response.read(MAX_RESPONSE_BYTES + 1)
        except urllib.error.HTTPError as error:
            raise ValueError(f"project API request failed with HTTP {error.code}") from None
        except (urllib.error.URLError, TimeoutError, OSError):
            raise ValueError("project API request failed") from None
        if len(body) > MAX_RESPONSE_BYTES:
            raise ValueError("project API response exceeds size limit")
        try:
            return json.loads(body, object_pairs_hook=unique_object, parse_constant=reject_constant)
        except (ValueError, UnicodeDecodeError, RecursionError):
            raise ValueError("project API returned invalid JSON") from None

    before_workspace = read("")
    before_workflow = read("/workflow")
    rows = inventory_rows(before_workspace, before_workflow, project_id)
    after_workflow = read("/workflow")
    after_workspace = read("")
    if (rows != inventory_rows(after_workspace, after_workflow, project_id)
            or before_workspace["version"] != after_workspace["version"]):
        raise ValueError("workflow or draft changed during export; reload and try again")
    return rows


def write_private_export(path, api_base, token, project_id):
    # Fetch and validate every proposal before creating/replacing any output file.
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
    parser.add_argument("--output", required=True)
    args = parser.parse_args(argv)
    if not positive_id(args.project_id):
        parser.error("project ID must be a positive signed 64-bit integer")
    try:
        count = write_private_export(args.output, args.api_base, args.token, args.project_id)
    except (OSError, ValueError, RecursionError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"Exported {count} Tester candidate identifiers; no review labels generated", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
