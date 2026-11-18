#!/usr/bin/env python3
"""Summarize observed Agent orchestration attempts without exporting raw artifacts."""

import argparse
import json
import os
from pathlib import Path
import sys
import tempfile
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode
from urllib.request import HTTPRedirectHandler, Request, build_opener

MAX_PAGE_SIZE = 100
MAX_ORCHESTRATIONS = 10_000
MAX_STEPS = 100_000
MAX_I64 = 9_223_372_036_854_775_807
DEFAULT_MINIMUM_CASES = 100
ORCHESTRATION_STATUSES = {"pending", "running", "succeeded", "failed", "cancelled"}
STEP_STATUSES = ORCHESTRATION_STATUSES


def wilson_interval(successes, denominator, z=1.959963984540054):
    if (type(successes) is not int or type(denominator) is not int
            or denominator < 0 or successes < 0 or successes > denominator):
        raise ValueError("Wilson interval requires 0 <= integer successes <= denominator")
    if denominator == 0:
        return None
    proportion = successes / denominator
    z_squared = z * z
    adjustment = 1 + z_squared / denominator
    center = (proportion + z_squared / (2 * denominator)) / adjustment
    margin = (z / adjustment) * (
        proportion * (1 - proportion) / denominator
        + z_squared / (4 * denominator * denominator)
    ) ** 0.5
    return {
        "lower": round(max(0.0, center - margin), 4),
        "upper": round(min(1.0, center + margin), 4),
    }


def rate_summary(successes, denominator):
    return {
        "successes": successes,
        "denominator": denominator,
        "value": round(successes / denominator, 4) if denominator else None,
        "wilson95": wilson_interval(successes, denominator),
    }


class RejectRedirects(HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        return None


def positive_id(value, field):
    if type(value) is not int or not 0 < value <= MAX_I64:
        raise ValueError(f"{field} must be a positive signed 64-bit integer")
    return value


def validate_usage(usage, context):
    if usage is None:
        return None
    if not isinstance(usage, dict):
        raise ValueError(f"{context} usage must be an object or null")
    tokens = {}
    for key in ("prompt_tokens", "completion_tokens", "total_tokens"):
        value = usage.get(key)
        if value is None:
            continue
        if type(value) is not int or value < 0 or value > MAX_I64:
            raise ValueError(f"{context} {key} must be a non-negative signed 64-bit integer")
        tokens[key] = value
    if "prompt_tokens" in tokens and "completion_tokens" in tokens:
        component_total = tokens["prompt_tokens"] + tokens["completion_tokens"]
        if component_total > MAX_I64:
            raise ValueError(f"{context} derived total_tokens exceeds signed 64-bit range")
        if "total_tokens" not in tokens:
            tokens["total_tokens"] = component_total
        elif tokens["total_tokens"] < component_total:
            raise ValueError(f"{context} total_tokens is less than prompt plus completion tokens")
    if "budget_reservation" in usage:
        if type(usage["budget_reservation"]) is not bool:
            raise ValueError(f"{context} budget_reservation must be a boolean")
        tokens["budget_reservation"] = usage["budget_reservation"]
    return tokens


def validate_history(history):
    if not isinstance(history, list):
        raise ValueError("orchestration history response must be an array")
    if len(history) > MAX_ORCHESTRATIONS:
        raise ValueError(f"history exceeds the {MAX_ORCHESTRATIONS} orchestration limit")
    seen_orchestrations = set()
    seen_step_ids = set()
    seen_step_attempts = set()
    step_count = 0
    validated = []
    previous_id = None
    for index, orchestration in enumerate(history):
        context = f"orchestration[{index}]"
        if not isinstance(orchestration, dict):
            raise ValueError(f"{context} must be an object")
        orchestration_id = positive_id(orchestration.get("id"), f"{context}.id")
        if previous_id is not None and orchestration_id >= previous_id:
            raise ValueError("orchestration IDs must be unique and descending")
        if orchestration_id in seen_orchestrations:
            raise ValueError(f"duplicate orchestration ID {orchestration_id}")
        previous_id = orchestration_id
        seen_orchestrations.add(orchestration_id)
        status = orchestration.get("status")
        if not isinstance(status, str) or status not in ORCHESTRATION_STATUSES:
            raise ValueError(f"{context}.status is unsupported")
        parent_orchestration_id = orchestration.get("parentOrchestrationId")
        retry_of_id = orchestration.get("retryOfOrchestrationId")
        if parent_orchestration_id is not None:
            positive_id(parent_orchestration_id, f"{context}.parentOrchestrationId")
            if parent_orchestration_id == orchestration_id:
                raise ValueError(f"{context} cannot be its own parent")
        if retry_of_id is not None:
            positive_id(retry_of_id, f"{context}.retryOfOrchestrationId")
            if retry_of_id == orchestration_id:
                raise ValueError(f"{context} cannot retry itself")
        raw_steps = orchestration.get("steps")
        if not isinstance(raw_steps, list):
            raise ValueError(f"{context}.steps must be an array")
        steps = []
        for step_index, step in enumerate(raw_steps):
            step_context = f"{context}.steps[{step_index}]"
            if not isinstance(step, dict):
                raise ValueError(f"{step_context} must be an object")
            step_id = positive_id(step.get("id"), f"{step_context}.id")
            if step_id in seen_step_ids:
                raise ValueError(f"duplicate step ID {step_id}")
            seen_step_ids.add(step_id)
            step_parent_id = positive_id(step.get("orchestrationId"), f"{step_context}.orchestrationId")
            if step_parent_id != orchestration_id:
                raise ValueError(f"{step_context} belongs to a different orchestration")
            step_no = positive_id(step.get("stepNo"), f"{step_context}.stepNo")
            attempt_no = positive_id(step.get("attemptNo"), f"{step_context}.attemptNo")
            agent_role = step.get("agentRole")
            if not isinstance(agent_role, str) or not agent_role.strip() or len(agent_role) > 64:
                raise ValueError(f"{step_context}.agentRole must be a non-empty string of at most 64 characters")
            task_kind = step.get("taskKind")
            if not isinstance(task_kind, str) or not task_kind.strip() or len(task_kind) > 128:
                raise ValueError(f"{step_context}.taskKind must be a non-empty string of at most 128 characters")
            model = step.get("model")
            if model is not None and (not isinstance(model, str) or len(model) > 255):
                raise ValueError(f"{step_context}.model must be a string of at most 255 characters or null")
            model = model.strip() if isinstance(model, str) else ""
            model = model or "unknown"
            identity = (orchestration_id, step_no, task_kind, agent_role, attempt_no)
            if identity in seen_step_attempts:
                raise ValueError(f"duplicate step attempt {identity}")
            seen_step_attempts.add(identity)
            step_status = step.get("status")
            if not isinstance(step_status, str) or step_status not in STEP_STATUSES:
                raise ValueError(f"{step_context}.status is unsupported")
            usage = validate_usage(step.get("usage"), step_context)
            steps.append({
                "id": step_id,
                "orchestrationId": orchestration_id,
                "stepNo": step_no,
                "attemptNo": attempt_no,
                "agentRole": agent_role,
                "taskKind": task_kind,
                "model": model,
                "status": step_status,
                "usage": usage,
            })
            step_count += 1
            if step_count > MAX_STEPS:
                raise ValueError(f"history exceeds the {MAX_STEPS} step limit")
        validated.append({
            "id": orchestration_id,
            "parentOrchestrationId": parent_orchestration_id,
            "retryOfOrchestrationId": retry_of_id,
            "status": status,
            "steps": steps,
        })
    return validated


def lineage_roots(orchestrations):
    """Topologically validate both edges and find roots without Python recursion."""
    by_id = {item["id"]: item for item in orchestrations}
    remaining = {}
    dependents = {orchestration_id: [] for orchestration_id in by_id}
    references = {}
    for item in orchestrations:
        item_references = set(reference for reference in (
            item["parentOrchestrationId"], item["retryOfOrchestrationId"]
        ) if reference is not None)
        for reference in item_references:
            if reference not in by_id:
                raise ValueError(f"orchestration {item['id']} references missing orchestration {reference}")
            dependents[reference].append(item["id"])
        references[item["id"]] = item_references
        remaining[item["id"]] = len(item_references)
    ready = [item_id for item_id, count in remaining.items() if count == 0]
    retry_roots = {}
    budget_roots = {}
    while ready:
        item_id = ready.pop()
        item = by_id[item_id]
        retry_of = item["retryOfOrchestrationId"]
        retry_roots[item_id] = retry_roots[retry_of] if retry_of is not None else item_id
        roots = {budget_roots[reference] for reference in references[item_id]}
        if len(roots) > 1:
            raise ValueError("orchestration parent/retry lineage joins multiple budget roots")
        budget_roots[item_id] = next(iter(roots)) if roots else item_id
        for dependent in dependents[item_id]:
            remaining[dependent] -= 1
            if remaining[dependent] == 0:
                ready.append(dependent)
    if len(retry_roots) != len(orchestrations):
        raise ValueError("orchestration parent/retry references contain a cycle")
    return retry_roots, budget_roots


def acceptance_status(successes, denominator, target, minimum_cases, complete=True):
    if denominator < minimum_cases:
        return "insufficient_samples"
    if not complete:
        return "incomplete_usage"
    return "target_met" if successes / denominator >= target else "target_not_met"


def orchestration_summary(orchestrations, minimum_cases):
    counts = {status: 0 for status in sorted(ORCHESTRATION_STATUSES)}
    for item in orchestrations:
        counts[item["status"]] += 1
    denominator = counts["succeeded"] + counts["failed"]
    return {
        "count": len(orchestrations),
        "statusCounts": counts,
        "inFlightCount": counts["pending"] + counts["running"],
        "success": {
            **rate_summary(counts["succeeded"], denominator),
            "targetRate": 0.95,
            "minimumCases": minimum_cases,
            "status": acceptance_status(counts["succeeded"], denominator, 0.95, minimum_cases),
        },
        "denominatorPolicy": "succeeded + failed orchestration records; cancellations and in-flight attempts are reported separately",
    }


def budget_tree_summary(orchestrations, retry_roots, budget_roots, token_budget, minimum_cases):
    retry_families = {}
    trees = {}
    for item in orchestrations:
        retry_families.setdefault(retry_roots[item["id"]], []).append(item)
        trees.setdefault(budget_roots[item["id"]], []).append(item)
    family_first = []
    family_latest = []
    family_last_by_root = {}
    for root, attempts in retry_families.items():
        attempts.sort(key=lambda item: item["id"])
        family_first.append(attempts[0])
        family_latest.append(attempts[-1])
        family_last_by_root[root] = attempts[-1]
    families = {
        "familyCount": len(retry_families),
        "retriedFamilyCount": sum(len(attempts) > 1 for attempts in retry_families.values()),
        "firstAttempt": orchestration_summary(family_first, minimum_cases),
        "latestAttempt": orchestration_summary(family_latest, minimum_cases),
        "policy": "retryOfOrchestrationId groups retries only; parent/child records stay separate families",
    }
    counts = {"succeeded": 0, "failed": 0, "cancelled": 0, "inFlight": 0}
    observed_trees = 0
    observed_tokens = 0
    within_budget = 0
    completed_within_budget = 0
    over_budget = 0
    for attempts in trees.values():
        # A terminal root alone does not close a tree whose child/retry is live.
        if any(item["status"] in ("pending", "running") for item in attempts):
            counts["inFlight"] += 1
            continue
        latest = {retry_roots[item["id"]]: family_last_by_root[retry_roots[item["id"]]] for item in attempts}
        statuses = {item["status"] for item in latest.values()}
        status = "cancelled" if "cancelled" in statuses else "failed" if "failed" in statuses else "succeeded"
        counts[status] += 1
        if status == "cancelled":
            continue
        total = 0
        complete = True
        for attempt in attempts:
            # The API's primary step stores the Agent-run cumulative usage.
            # Architect/validator/tester bookkeeping steps can have no usage;
            # summing all steps would double count cumulative rollups.
            primary = [step for step in attempt["steps"] if step["stepNo"] == 1]
            if len(primary) != 1 or primary[0]["status"] in ("pending", "running"):
                complete = False
                break
            usage = primary[0]["usage"]
            if usage is None or "total_tokens" not in usage or usage.get("budget_reservation", False):
                complete = False
                break
            total += usage["total_tokens"]
        if not complete:
            continue
        observed_trees += 1
        observed_tokens += total
        if token_budget is not None:
            if total <= token_budget:
                within_budget += 1
                completed_within_budget += status == "succeeded"
            else:
                over_budget += 1
    denominator = counts["succeeded"] + counts["failed"]
    status = "not_configured" if token_budget is None else acceptance_status(
        completed_within_budget, denominator, 0.90, minimum_cases,
        complete=observed_trees == denominator,
    )
    budget = {
        "treeCount": len(trees),
        "statusCounts": counts,
        "evaluableTrees": denominator,
        "tokenObservedTrees": observed_trees,
        "tokenMissingTrees": denominator - observed_trees,
        "tokenCoverage": rate_summary(observed_trees, denominator),
        "observedTokenTotal": observed_tokens,
        "thresholdTokens": token_budget,
        "withinBudgetTrees": within_budget if token_budget is not None else None,
        "overBudgetTrees": over_budget if token_budget is not None else None,
        "completedWithinBudget": {
            **(rate_summary(completed_within_budget, observed_trees) if token_budget is not None else {
                "successes": None, "denominator": observed_trees, "value": None, "wilson95": None,
            }),
            "targetRate": 0.90,
            "minimumCases": minimum_cases,
            "status": status,
        },
        "completedWithinBudgetLowerBound": rate_summary(completed_within_budget, denominator) if token_budget is not None else None,
        "usageSource": "one primary stepNo=1 Agent-run rollup per orchestration record; all records in parent+retry tree",
        "denominatorPolicy": "closed trees with final retry-family outcomes succeeded/failed; missing usage stays unknown and prevents target_met",
        "thresholdPolicy": "explicit uniform observation threshold, not a historical deployment configuration snapshot or hard provider token cap",
    }
    return families, budget


def role_flow_summary(orchestrations, budget_roots):
    core_roles = ("analyst", "architect", "engineer", "tester")
    trees = {}
    for item in orchestrations:
        tree = trees.setdefault(budget_roots[item["id"]], {
            "attempted": set(),
            "succeeded": set(),
        })
        for step in item["steps"]:
            role = step["agentRole"]
            tree["attempted"].add(role)
            if step["status"] == "succeeded":
                tree["succeeded"].add(role)

    eligible = [
        tree for tree in trees.values()
        if tree["attempted"].intersection(("analyst", "architect", "engineer"))
    ]
    complete = [tree for tree in eligible if set(core_roles).issubset(tree["succeeded"])]
    role_counts = {}
    for role in (*core_roles, "facilitator"):
        role_counts[role] = {
            "attemptedTrees": sum(role in tree["attempted"] for tree in eligible),
            "succeededTrees": sum(role in tree["succeeded"] for tree in eligible),
        }
    return {
        "workflowTreeCount": len(eligible),
        "coreRoles": list(core_roles),
        "completeCoreRoleTrees": len(complete),
        "coreRoleCompletion": rate_summary(len(complete), len(eligible)),
        "roles": role_counts,
        "policy": "parent and retry edges define a tree; a role succeeds if any step for that role succeeded in the tree; Facilitator is conditional and excluded from the core completion requirement",
    }


def evaluate(history, token_budget=None, minimum_cases=DEFAULT_MINIMUM_CASES):
    if type(minimum_cases) is not int or not 1 <= minimum_cases <= MAX_ORCHESTRATIONS:
        raise ValueError(f"minimum_cases must be between 1 and {MAX_ORCHESTRATIONS}")
    if token_budget is not None:
        positive_id(token_budget, "token_budget")
    orchestrations = validate_history(history)
    retry_roots, budget_roots = lineage_roots(orchestrations)
    by_id = {item["id"]: item for item in orchestrations}
    task_groups = {}
    model_groups = {}
    attempt_count = 0
    retry_attempt_count = 0
    retried_logical_steps = 0
    logical_steps = {}
    for orchestration in orchestrations:
        for step in orchestration["steps"]:
            task_kind = step["taskKind"]
            group = task_groups.setdefault(task_kind, {
                "logicalSteps": 0,
                "firstAttemptEvaluableSteps": 0,
                "firstAttemptSucceeded": 0,
                "firstAttemptFailed": 0,
                "firstAttemptCancelled": 0,
                "finalAttemptEvaluableSteps": 0,
                "finalAttemptSucceeded": 0,
                "finalAttemptFailed": 0,
                "finalAttemptCancelled": 0,
                "cancelledAttempts": 0,
                "attempts": 0,
                "retryAttempts": 0,
                "retriedLogicalSteps": 0,
                "tokenObservedAttempts": 0,
                "tokenTotal": 0,
                "promptTokens": 0,
                "completionTokens": 0,
            })
            model_groups.setdefault(step["model"], {
                "attempts": 0,
                "retryAttempts": 0,
                "firstAttemptEvaluableSteps": 0,
                "firstAttemptSucceeded": 0,
                "firstAttemptFailed": 0,
                "firstAttemptCancelled": 0,
                "finalAttemptEvaluableSteps": 0,
                "finalAttemptSucceeded": 0,
                "finalAttemptFailed": 0,
                "finalAttemptCancelled": 0,
                "cancelledAttempts": 0,
                "tokenObservedAttempts": 0,
                "tokenTotal": 0,
                "promptTokens": 0,
                "completionTokens": 0,
            })
            key = (
                retry_roots[orchestration["id"]],
                step["stepNo"],
                task_kind,
                step["agentRole"],
            )
            logical_steps.setdefault(key, []).append(step)
    for key, attempts in logical_steps.items():
        _, _, task_kind, _ = key
        attempts.sort(key=lambda item: (item["orchestrationId"], item["attemptNo"]))
        group = task_groups[task_kind]
        by_orchestration = {}
        for attempt in attempts:
            by_orchestration.setdefault(attempt["orchestrationId"], []).append(attempt)
        for local_attempts in by_orchestration.values():
            local_attempts.sort(key=lambda item: item["attemptNo"])
            first_number = local_attempts[0]["attemptNo"]
            if first_number != 1 and by_id[local_attempts[0]["orchestrationId"]]["retryOfOrchestrationId"] is None:
                raise ValueError("logical step attempts must start at attemptNo 1 unless this is a retry")
            if [item["attemptNo"] for item in local_attempts] != list(range(first_number, first_number + len(local_attempts))):
                raise ValueError("logical step attempts must be contiguous within an orchestration")
        group["logicalSteps"] += 1
        group["attempts"] += len(attempts)
        attempt_count += len(attempts)
        retries = max(0, len(attempts) - 1)
        group["retryAttempts"] += retries
        retry_attempt_count += retries
        if retries:
            group["retriedLogicalSteps"] += 1
            retried_logical_steps += 1
        first = attempts[0]
        if first["attemptNo"] != 1:
            raise ValueError("each logical step must include attemptNo 1")
        first_model = model_groups[first["model"]]
        if first["status"] in ("succeeded", "failed"):
            group["firstAttemptEvaluableSteps"] += 1
            group[f"firstAttempt{first['status'].capitalize()}"] += 1
            first_model["firstAttemptEvaluableSteps"] += 1
            first_model[f"firstAttempt{first['status'].capitalize()}"] += 1
        elif first["status"] == "cancelled":
            group["firstAttemptCancelled"] += 1
            first_model["firstAttemptCancelled"] += 1
        final = attempts[-1]
        final_model = model_groups[final["model"]]
        if final["status"] in ("succeeded", "failed"):
            group["finalAttemptEvaluableSteps"] += 1
            final_model["finalAttemptEvaluableSteps"] += 1
            if final["status"] == "succeeded":
                group["finalAttemptSucceeded"] += 1
                final_model["finalAttemptSucceeded"] += 1
            else:
                group["finalAttemptFailed"] += 1
                final_model["finalAttemptFailed"] += 1
        elif final["status"] == "cancelled":
            group["finalAttemptCancelled"] += 1
            final_model["finalAttemptCancelled"] += 1
        for attempt in attempts:
            model_group = model_groups[attempt["model"]]
            model_group["attempts"] += 1
            if attempt is not first:
                model_group["retryAttempts"] += 1
            if attempt["status"] == "cancelled":
                group["cancelledAttempts"] += 1
                model_group["cancelledAttempts"] += 1
            usage = attempt["usage"]
            if usage is None or "total_tokens" not in usage:
                continue
            group["tokenObservedAttempts"] += 1
            group["tokenTotal"] += usage["total_tokens"]
            group["promptTokens"] += usage.get("prompt_tokens", 0)
            group["completionTokens"] += usage.get("completion_tokens", 0)
            model_group["tokenObservedAttempts"] += 1
            model_group["tokenTotal"] += usage["total_tokens"]
            model_group["promptTokens"] += usage.get("prompt_tokens", 0)
            model_group["completionTokens"] += usage.get("completion_tokens", 0)

    per_task = {}
    for task_kind in sorted(task_groups):
        group = task_groups[task_kind]
        first_denominator = group["firstAttemptEvaluableSteps"]
        final_denominator = group["finalAttemptEvaluableSteps"]
        per_task[task_kind] = {
            **group,
            "firstAttemptSuccess": rate_summary(
                group["firstAttemptSucceeded"], first_denominator
            ),
            "firstAttemptFailure": rate_summary(
                group["firstAttemptFailed"], first_denominator
            ),
            "finalAttemptSuccess": rate_summary(
                group["finalAttemptSucceeded"], final_denominator
            ),
            "finalAttemptFailure": rate_summary(
                group["finalAttemptFailed"], final_denominator
            ),
            "firstAttemptSuccessRate": (
                round(group["firstAttemptSucceeded"] / first_denominator, 4)
                if first_denominator else None
            ),
            "finalAttemptSuccessRate": (
                round(group["finalAttemptSucceeded"] / final_denominator, 4)
                if final_denominator else None
            ),
        }
    per_model = {}
    for model in sorted(model_groups):
        group = model_groups[model]
        first_denominator = group["firstAttemptEvaluableSteps"]
        final_denominator = group["finalAttemptEvaluableSteps"]
        per_model[model] = {
            **group,
            "firstAttemptSuccess": rate_summary(
                group["firstAttemptSucceeded"], first_denominator
            ),
            "firstAttemptFailure": rate_summary(
                group["firstAttemptFailed"], first_denominator
            ),
            "finalAttemptSuccess": rate_summary(
                group["finalAttemptSucceeded"], final_denominator
            ),
            "finalAttemptFailure": rate_summary(
                group["finalAttemptFailed"], final_denominator
            ),
            "firstAttemptSuccessRate": (
                round(group["firstAttemptSucceeded"] / first_denominator, 4)
                if first_denominator else None
            ),
            "finalAttemptSuccessRate": (
                round(group["finalAttemptSucceeded"] / final_denominator, 4)
                if final_denominator else None
            ),
        }
    retry_families, budget_trees = budget_tree_summary(
        orchestrations, retry_roots, budget_roots, token_budget, minimum_cases,
    )
    role_flow = role_flow_summary(orchestrations, budget_roots)
    return {
        "orchestrationAttempts": orchestration_summary(orchestrations, minimum_cases),
        "retryFamilies": retry_families,
        "budgetTrees": budget_trees,
        "roleFlow": role_flow,
        "scope": "observed orchestration history; completed first attempts only for first-attempt success rate",
        "orchestrationCount": len(orchestrations),
        "stepAttemptCount": attempt_count,
        "retryAttemptCount": retry_attempt_count,
        "retriedLogicalStepCount": retried_logical_steps,
        "taskKinds": per_task,
        "models": per_model,
        "limitation": "These are observed persisted runs, not a controlled model-quality evaluation. Partial windows, in-flight runs, missing usage, model routing, and task-mix changes affect comparisons. Budget-tree usage follows the current primary-step Agent-run rollup convention; historical token thresholds and provider billing completeness are not recorded by this API. Wilson intervals do not adjust retry/tree correlation or sampling bias.",
    }


def open_url(request, timeout):
    return build_opener(RejectRedirects()).open(request, timeout=timeout)


def fetch_history(api_base, token, project_id, page_size=100):
    positive_id(project_id, "project_id")
    if not isinstance(token, str) or not token.strip():
        raise ValueError("an API bearer token is required")
    if type(page_size) is not int or not 1 <= page_size <= MAX_PAGE_SIZE:
        raise ValueError(f"page size must be between 1 and {MAX_PAGE_SIZE}")
    base_url = api_base.rstrip("/")
    before_id = None
    history = []
    seen_ids = set()
    while True:
        query = {"limit": page_size}
        if before_id is not None:
            query["beforeId"] = before_id
        url = f"{base_url}/api/projects/{project_id}/orchestrations?{urlencode(query)}"
        request = Request(url, headers={"Authorization": f"Bearer {token}", "Accept": "application/json"})
        try:
            with open_url(request, timeout=30) as response:
                raw = response.read(4_194_305)
        except (HTTPError, URLError, TimeoutError) as error:
            raise RuntimeError(f"failed to read orchestration history before ID {before_id}: {error}") from error
        if len(raw) > 4_194_304:
            raise RuntimeError("orchestration history page exceeds 4 MiB")
        try:
            page = json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise RuntimeError("orchestration history response is not valid JSON") from error
        if not isinstance(page, list) or len(page) > page_size:
            raise RuntimeError("orchestration history API returned an invalid page")
        if not page:
            break
        ids = []
        for item in page:
            if not isinstance(item, dict):
                raise RuntimeError("orchestration history API returned a non-object item")
            orchestration_id = positive_id(item.get("id"), "orchestration id")
            if orchestration_id in seen_ids:
                raise RuntimeError(f"orchestration history API repeated ID {orchestration_id}")
            if before_id is not None and orchestration_id >= before_id:
                raise RuntimeError("orchestration history API did not advance its cursor")
            ids.append(orchestration_id)
            seen_ids.add(orchestration_id)
        if any(left <= right for left, right in zip(ids, ids[1:])):
            raise RuntimeError("orchestration history page IDs are not strictly descending")
        history.extend(page)
        if len(history) > MAX_ORCHESTRATIONS:
            raise RuntimeError(f"history exceeds the {MAX_ORCHESTRATIONS} orchestration limit")
        if len(page) < page_size:
            break
        before_id = ids[-1]
    return history


def write_report(report, output_path):
    output = Path(output_path)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_path = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=output.parent, prefix=f".{output.name}.", delete=False
        ) as target:
            temporary_path = target.name
            os.chmod(temporary_path, 0o600)
            json.dump(report, target, ensure_ascii=False, indent=2)
            target.write("\n")
        os.replace(temporary_path, output)
    except OSError:
        if temporary_path and os.path.exists(temporary_path):
            os.unlink(temporary_path)
        raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project_id", type=int)
    parser.add_argument("--api-base", default=os.environ.get("API_BASE", "http://127.0.0.1:3000"))
    parser.add_argument("--token", default=os.environ.get("API_TOKEN"), required="API_TOKEN" not in os.environ)
    parser.add_argument("--page-size", type=int, default=100)
    parser.add_argument("--output", help="write the aggregate report to this path instead of stdout")
    parser.add_argument("--token-budget", type=int, help="uniform observed-token threshold for each complete parent/retry budget tree")
    parser.add_argument("--minimum-cases", type=int, default=DEFAULT_MINIMUM_CASES,
                        help="minimum evaluable records/trees for acceptance status (default: 100)")
    args = parser.parse_args(argv)
    try:
        if args.token_budget is not None:
            positive_id(args.token_budget, "token_budget")
        if not 1 <= args.minimum_cases <= MAX_ORCHESTRATIONS:
            raise ValueError(f"minimum_cases must be between 1 and {MAX_ORCHESTRATIONS}")
        history = fetch_history(args.api_base, args.token, args.project_id, args.page_size)
        report = evaluate(history, args.token_budget, args.minimum_cases)
        rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
        if args.output:
            write_report(report, args.output)
        else:
            sys.stdout.write(rendered)
    except (HTTPError, URLError, OSError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"evaluated {len(history)} Agent orchestrations", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
