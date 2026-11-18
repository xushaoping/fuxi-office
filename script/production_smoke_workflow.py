"""Deterministic API governance fixture. No requirement analysis or model calls."""
import json
import secrets

DSL = '''name: ReleaseSmoke
inputs:
  - name: Age
    typeRef: number
decisions:
  - name: Result
    typeRef: string
    requiredInputs: [Age]
    logic:
      type: decisionTable
      hitPolicy: FIRST
      inputs:
        - expression: Age
          typeRef: number
      outputs:
        - name: result
          typeRef: string
      rules:
        - inputEntries: [">= 18"]
          outputEntries: ['"Approve"']
        - inputEntries: ["-"]
          outputEntries: ['"Reject"']
'''


def run_workflow(request, token_for, on_project=None):
    tokens = {role: token_for("governance-" + role) for role in
              ("owner", "modeler", "reviewer", "approver", "viewer", "outsider")}

    def call(path, method="GET", actor="modeler", data=None, expected=200):
        payload = request(path, method, tokens[actor], data, expected)
        return json.loads(payload) if payload else None

    for role in tokens:
        request("/api/admin/users", "POST", token_for("admin"),
                {"username": "governance-" + role, "password": secrets.token_hex(16), "role": "user"}, 201)
    project = call("/api/projects", "POST", "owner", {"name": "isolated governance smoke", "riskLevel": "low"}, 201)
    prefix = f'/api/projects/{project["id"]}'
    for role in ("modeler", "reviewer", "approver", "viewer"):
        call(prefix + "/members/governance-" + role, "PUT", "owner", {"role": role})
    call(prefix, actor="outsider", expected=404)
    xml = call("/api/compile", "POST", data={"dsl": DSL})["xml"]
    version_payload = {"dslYaml": DSL, "dmnXml": xml, "changeConfirmed": True}
    call(prefix + "/versions", "POST", "viewer", version_payload, 404)
    version = call(prefix + "/versions", "POST", data=version_payload, expected=201)
    vp = prefix + f'/versions/{version["id"]}'
    # Read-only change details must remain available to the creator and viewer.
    call(vp + "/review")
    call(vp + "/review", actor="viewer")
    case = call(vp + "/test-cases", "POST", data={"name": "adult boundary", "inputJson": {"Age": 18},
                "expectedOutputJson": {"Result": "Approve"}}, expected=201)
    cases = call(vp + "/test-cases/run", "POST")
    if not cases or any(item["status"] != "passed" for item in cases):
        raise RuntimeError("deterministic business case failed")
    browser = on_project(project, tokens) if on_project else None
    approval = vp + f'/test-cases/{case["id"]}/approval'
    call(approval, "PUT", data={"approved": True}, expected=403)
    call(approval, "PUT", "reviewer", {"approved": True})
    call(vp + "/review-approval", "POST", expected=403)
    call(prefix + "/members/governance-modeler", "PUT", "owner", {"role": "reviewer"})
    call(vp + "/review-approval", "POST", expected=400)  # Creator identity still blocks delegated review.
    call(prefix + "/members/governance-modeler?role=reviewer", "DELETE", "owner", expected=204)
    call(vp + "/review-approval", "POST", "reviewer")
    verification = call(vp + "/verification", "POST")
    if verification["status"] != "passed":
        failed = [item.get("name", item.get("code")) for item in verification.get("gates", [])
                  if item.get("status") == "failed"]
        raise RuntimeError(f"deterministic verification failed: {failed}")
    call(vp + "/publish", "POST", "reviewer", {"publishConfirmed": True}, 403)
    call(vp + "/publish", "POST", "viewer", {"publishConfirmed": True}, 403)
    for actor in ("modeler", "reviewer"):
        call(prefix + "/members/governance-" + actor, "PUT", "owner", {"role": "approver"})
        call(vp + "/publish", "POST", actor, {"publishConfirmed": True}, 403)
        call(prefix + "/members/governance-" + actor + "?role=approver", "DELETE", "owner", expected=204)
    call(vp + "/publish", "POST", "approver", {"publishConfirmed": True})
    services = call(prefix + "/decision-services", actor="viewer")
    if not services:
        raise RuntimeError("published fixture has no executable service")
    service_id = services[0]["id"]
    credential = {"serviceId": service_id, "name": "isolated fixture key"}
    call(prefix + "/decision-service-keys", "POST", "reviewer", credential, 404)
    key = call(prefix + "/decision-service-keys", "POST", "approver", credential)
    # Verify the operator's existing permission issue as both denial and success.
    if not key.get("secret") or key["key"]["serviceId"] != service_id:
        raise RuntimeError("approved operator did not receive a scoped service credential")
    external_path = f'/api/public/projects/{project["id"]}/decision-services/execute'
    external_payload = {"serviceId": service_id, "inputs": {"Age": 18}}
    external = json.loads(request(external_path, "POST", data=external_payload,
                                  extra_headers={"X-Decision-Key": key["secret"]}))
    if external["status"] != "succeeded":
        raise RuntimeError("scoped external service credential could not execute its service")
    result = call(prefix + "/decision-services/execute", "POST", "viewer",
                  {"serviceId": service_id, "inputs": {"Age": 18}})
    if result["status"] != "succeeded":
        raise RuntimeError("published deterministic service execution failed")
    call(prefix + f'/decision-service-keys/{key["key"]["id"]}', "DELETE", "approver", expected=204)
    request(external_path, "POST", data=external_payload, expected=401,
            extra_headers={"X-Decision-Key": key["secret"]})
    call(prefix + "/members/governance-viewer", "DELETE", "owner", expected=204)
    call(prefix, actor="viewer", expected=404)  # Membership changes apply to existing JWTs.
    call(prefix, "DELETE", "owner", expected=204)
    return {"status": "passed", "checks": ["draft", "read_change_details", "business_case",
            "independent_review", "verification", "independent_publish", "identity_separation_with_multiple_roles", "service_execution",
            "service_credential_permissions", "external_execution_and_revocation", "membership_revocation", "project_lifecycle"],
            "externalModelCalls": 0, "browser": browser,
            "scope": "deterministic API fixture; not provider or model-quality acceptance"}
