#!/usr/bin/env python3
"""Exercise a production image in an owned disposable Compose project, without external AI calls."""

import argparse
import base64
import hashlib
import hmac
import importlib.util
import json
import os
import re
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


def fixture_token(username, secret):
    def encode(value):
        raw = json.dumps(value, separators=(",", ":")).encode()
        return base64.urlsafe_b64encode(raw).decode().rstrip("=")
    message = encode({"alg": "HS256", "typ": "JWT"}) + "." + encode(
        {"sub": username, "exp": int(time.time()) + 900})
    signature = hmac.new(secret.encode(), message.encode(), hashlib.sha256).digest()
    return message + "." + base64.urlsafe_b64encode(signature).decode().rstrip("=")


REQUIRED_ASSETS = {"pkg/fuxi-dmn.js", "pkg/fuxi-dmn.wasm", "pkg/fuxi-dmn.css"}


def isolated_environment():
    # Use an allowlist: a developer shell may contain production DB, model,
    # Docker/Compose override and proxy configuration unknown to this script.
    allowed = {"PATH", "HOME", "USER", "LOGNAME", "TMPDIR", "TEMP", "TMP",
               "SYSTEMROOT", "WINDIR", "XDG_RUNTIME_DIR"}
    return {key: value for key, value in os.environ.items() if key in allowed}


def local_image(image, environment):
    if not isinstance(image, str) or not image or image.startswith("-"):
        raise ValueError("an already-built local image is required")
    result = subprocess.run(["docker", "image", "inspect", "--format", "{{json .}}", image],
                            env=environment, capture_output=True, text=True, timeout=20)
    if result.returncode:
        raise RuntimeError("application image is not available locally; build/load it explicitly before smoke testing")
    try:
        metadata = json.loads(result.stdout)
        image_id, arch = metadata["Id"], metadata["Architecture"]
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", image_id):
            raise ValueError("invalid image ID")
        if metadata.get("Os") != "linux" or arch not in {"amd64", "arm64"}:
            raise ValueError("unsupported image platform")
    except (KeyError, TypeError, ValueError) as error:
        raise RuntimeError("local image metadata is invalid or not a supported Linux image") from error
    return image_id, "linux/" + arch


def verified_asset_entries(manifest):
    if not isinstance(manifest, dict):
        raise RuntimeError("release manifest must be an object")
    version = manifest.get("assetVersion")
    if not isinstance(version, str) or not re.fullmatch(r"[0-9a-f]{64}", version):
        raise RuntimeError("release manifest is missing its asset digest")
    entries = manifest.get("assets")
    if not isinstance(entries, list):
        raise RuntimeError("release manifest is missing its asset inventory")
    required = {}
    for entry in entries:
        if not isinstance(entry, dict):
            raise RuntimeError("release manifest contains an invalid asset entry")
        path = entry.get("path")
        if not isinstance(path, str):
            raise RuntimeError("release manifest contains an invalid asset path")
        if path not in REQUIRED_ASSETS:
            continue
        if path in required:
            raise RuntimeError("release manifest duplicates a required asset")
        digest = entry.get("sha256")
        size = entry.get("size")
        if (not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest)
                or not isinstance(size, int) or isinstance(size, bool) or size <= 0):
            raise RuntimeError("release manifest contains an invalid asset digest or size")
        required[path] = entry
    if set(required) != REQUIRED_ASSETS:
        raise RuntimeError("release manifest must contain all three required JS/WASM/CSS assets")
    return version, required


def source_eligibility(manifest, required=False):
    source = manifest.get("source")
    source = source if isinstance(source, dict) else {}
    commit = source.get("commit")
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", commit):
        commit = None
    eligible = (manifest.get("productionSourceEligible") is True
                and source.get("workingTreeClean") is True and commit is not None)
    if required and not eligible:
        raise RuntimeError("production smoke requires a clean, tracked source revision in the release manifest")
    return commit, eligible


def browser_environment(environment, base_url, tokens, project_name, screenshot_dir):
    result = dict(environment,
                  FUXI_BROWSER_BASE_URL=base_url,
                  FUXI_BROWSER_TOKENS_JSON=json.dumps(tokens),
                  FUXI_BROWSER_PROJECT_NAME=project_name,
                  FUXI_BROWSER_SCREENSHOT_DIR=str(screenshot_dir))
    # Keep the smoke isolated while passing only the paths needed by Playwright.
    for key in ("PLAYWRIGHT_CORE_PATH", "PLAYWRIGHT_BROWSERS_PATH"):
        value = os.environ.get(key)
        if value:
            result[key] = value
    return result


def run_smoke(image, timeout=240, require_production_source=False, workflow=False, browser=False):
    if not isinstance(timeout, int) or not 30 <= timeout <= 600:
        raise ValueError("timeout must be between 30 and 600 seconds")
    root = Path(__file__).resolve().parents[1]
    environment = isolated_environment()
    # Inspect before creating resources. Pin the resolved ID so a tag cannot
    # change between inspection and startup; no implicit registry pull is allowed.
    image_id, platform = local_image(image, environment)
    project = "fuxi-release-smoke-" + secrets.token_hex(6)
    # Temporary fixture credentials must not reuse an operator's production settings.
    jwt_secret = secrets.token_hex(32)
    fixture_env = {
        "FUXI_IMAGE": image_id, "APP_PLATFORM": platform, "APP_PORT": "0",
        "APP_ENV": "production", "APP_DB_USER": "fuxi", "APP_DB_OWNER": "fuxi_owner",
        "DB_ADMIN_PASSWORD": secrets.token_hex(24), "DB_PASSWORD": secrets.token_hex(24),
        "JWT_SECRET": jwt_secret, "ADMIN_PASSWORD": secrets.token_hex(24),
        "OPENAI_API_KEY": "", "FUXI_EMBEDDING_ENABLED": "false", "EMBEDDING_ENABLED": "false",
        "OPTIMIZATION_AUTODRAFT_MODE": "off", "TRUST_PROXY_HEADERS": "false",
    }
    with tempfile.TemporaryDirectory(prefix=project + "-") as temporary:
        env_path = Path(temporary) / "fixture.env"
        env_path.write_text("".join(f"{key}={value}\n" for key, value in fixture_env.items()))
        env_path.chmod(0o600)
        command = ["docker", "compose", "--project-name", project, "--env-file", str(env_path),
                   "-f", str(root / "docker-compose.multi.yml")]

        def compose(*args, check=True, limit=timeout):
            result = subprocess.run(command + list(args), cwd=root, env=environment,
                                    capture_output=True, text=True, timeout=limit)
            if check and result.returncode:
                # Do not print env-file contents or container logs containing model data.
                raise RuntimeError(f"isolated Compose operation failed: {args[0]} (exit {result.returncode})")
            return result

        class LocalOnlyRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, newurl):
                # Every smoke endpoint is canonical; following redirects could
                # leave the isolated local deployment.
                return None

        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), LocalOnlyRedirect())
        base = None
        invalid_container = project + "-invalid-startup"
        invalid_attempted = False

        def request(path, method="GET", token=None, data=None, expected=200, extra_headers=None):
            headers = {"Accept": "application/json"}
            if token:
                headers["Authorization"] = "Bearer " + token
            headers.update(extra_headers or {})
            body = None
            if data is not None:
                headers["Content-Type"] = "application/json"
                body = json.dumps(data).encode()
            req = urllib.request.Request(base + path, data=body, headers=headers, method=method)
            try:
                with opener.open(req, timeout=10) as response:
                    status, payload = response.status, response.read(16 * 1024 * 1024)
            except urllib.error.HTTPError as error:
                status, payload = error.code, error.read(4096)
            if status != expected:
                raise RuntimeError(f"isolated smoke {method} {path}: expected {expected}, got {status}")
            return payload

        def eventually(path, expected, seconds=30):
            deadline = time.monotonic() + seconds
            last_error = None
            while time.monotonic() < deadline:
                try:
                    return request(path, expected=expected)
                except (RuntimeError, urllib.error.URLError, TimeoutError) as error:
                    last_error = error
                    time.sleep(0.25)
            raise RuntimeError(f"isolated smoke timed out checking {path}: {last_error}")

        try:
            compose("up", "-d", "--no-build", "--pull", "never", "--wait", "--wait-timeout", str(timeout), "postgres", "app")
            mapping = compose("port", "app", "3000").stdout.strip()
            match = re.fullmatch(r"127\.0\.0\.1:([0-9]+)", mapping)
            if not match or not 1 <= int(match[1]) <= 65535:
                raise RuntimeError("could not resolve isolated loopback application port")
            base = "http://127.0.0.1:" + match[1]
            eventually("/api/ready", 200)
            request("/api/health")
            manifest = json.loads(request("/release-manifest.json"))
            version, assets = verified_asset_entries(manifest)
            source_commit, source_eligible = source_eligibility(manifest, require_production_source)
            shell = request("/").decode()
            for name in sorted(REQUIRED_ASSETS):
                if f"/{name}?v={version}" not in shell:
                    raise RuntimeError("served shell and release assets have different versions")
            for path, entry in assets.items():
                payload = request("/" + path)
                if len(payload) != entry["size"] or hashlib.sha256(payload).hexdigest() != entry["sha256"]:
                    raise RuntimeError("served release asset failed its digest/size check")

            # JWTs are signed only with this disposable deployment's secret. This
            # exercises authorization, and is deliberately not a CAPTCHA/login test.
            owner_token = fixture_token("admin", jwt_secret)
            request("/api/me", token=owner_token)
            for username in ("smoke-viewer", "smoke-outsider"):
                request("/api/admin/users", "POST", owner_token,
                        {"username": username, "password": secrets.token_hex(16), "role": "user"}, 201)
            project_data = json.loads(request("/api/projects", "POST", owner_token,
                                             {"name": "isolated production smoke", "description": "disposable API fixture",
                                              "riskLevel": "low"}, 201))
            project_id = project_data["id"]
            prefix = f"/api/projects/{project_id}"
            request(prefix + "/members/smoke-viewer", "PUT", owner_token, {"role": "viewer"})
            viewer_token = fixture_token("smoke-viewer", jwt_secret)
            outsider_token = fixture_token("smoke-outsider", jwt_secret)
            request(prefix + "/deviation-analysis-inventory", token=viewer_token)
            request(prefix + "/deviation-analysis-inventory", token=outsider_token, expected=404)
            request(prefix, "PUT", viewer_token,
                    {"name": "viewer cannot change project", "description": "authorization fixture",
                     "riskLevel": "low"}, 404)

            workflow_report = None
            if workflow:
                spec = importlib.util.spec_from_file_location("production_smoke_workflow",
                    root / "script/production_smoke_workflow.py")
                module = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(module)

                def browser_smoke(project, tokens):
                    if not browser:
                        return None
                    node = shutil.which("node", path=environment.get("PATH"))
                    if not node:
                        raise RuntimeError("--browser requires Node.js")
                    browser_env = browser_environment(environment, base, tokens, project["name"],
                                                      root / "target/test-results")
                    result = subprocess.run([node, str(root / "script/smoke-production-browser.cjs")],
                                            cwd=temporary, env=browser_env, capture_output=True, text=True,
                                            timeout=120)
                    if result.returncode:
                        (root / "target/test-results").mkdir(parents=True, exist_ok=True)
                        (root / "target/test-results/production-image-browser.log").write_text(
                            result.stdout + result.stderr, encoding="utf-8")
                        raise RuntimeError("isolated image browser fixture failed; inspect target/test-results/production-image-browser.log")
                    return json.loads(result.stdout)

                workflow_report = module.run_workflow(request, lambda user: fixture_token(user, jwt_secret),
                                                      browser_smoke if browser else None)

            compose("exec", "-T", "postgres", "psql", "-X", "-v", "ON_ERROR_STOP=1", "-U", "postgres",
                    "-d", "fuxi_dmn", "-c",
                    "ALTER TABLE decision_run_explanations RENAME COLUMN generation_revision TO smoke_hidden_revision")
            try:
                eventually("/api/ready", 503)
                request("/api/health")
            finally:
                compose("exec", "-T", "postgres", "psql", "-X", "-v", "ON_ERROR_STOP=1", "-U", "postgres",
                        "-d", "fuxi_dmn", "-c",
                        "ALTER TABLE decision_run_explanations RENAME COLUMN smoke_hidden_revision TO generation_revision")
            eventually("/api/ready", 200)
            compose("stop", "postgres")
            eventually("/api/ready", 503)
            request("/api/health")
            compose("start", "postgres")
            eventually("/api/ready", 200, seconds=60)
            container_id = compose("ps", "-q", "app").stdout.strip()
            if not re.fullmatch(r"[0-9a-f]{12,64}", container_id):
                raise RuntimeError("could not resolve owned application container")
            compose("stop", "app")
            exit_status = subprocess.run(["docker", "inspect", "--format", "{{.State.ExitCode}}", container_id],
                                         env=environment, capture_output=True, text=True, check=True,
                                         timeout=20).stdout.strip()
            if exit_status != "0":
                raise RuntimeError("isolated application did not exit cleanly on SIGTERM")
            invalid_attempted = True
            rejected = subprocess.run(["docker", "run", "--pull=never", "--name", invalid_container,
                                       "--network", "none", "-e", "APP_ENV=production", image_id],
                                      env=environment, capture_output=True, text=True, timeout=20)
            # Docker's own 125/126/127 errors do not demonstrate application rejection.
            if rejected.returncode != 1:
                raise RuntimeError("production image did not reject missing startup configuration with application exit 1")
            return {"status": "passed", "image": image, "imageId": image_id, "platform": platform,
                    "assetVersion": version, "sourceCommit": source_commit,
                    "productionSourceEligible": source_eligible,
                    "workflow": workflow_report,
                    "browserEnabled": browser,
                    "checks": ["ready", "served_asset_hashes", "project_authorization",
                               "missing_schema_unready", "database_outage_and_recovery", "sigterm", "invalid_startup"],
                    "externalModelCalls": 0,
                    "scope": "disposable image deployment/API smoke; browser mode checks login PoW rejection, WASM and authenticated role pages, not provider, load or business-quality acceptance"}
        finally:
            cleanup_errors = []
            if invalid_attempted:
                try:
                    removed = subprocess.run(["docker", "rm", "--force", invalid_container],
                                             env=environment, capture_output=True, text=True, timeout=30)
                    if removed.returncode:
                        cleanup_errors.append("invalid-startup container")
                except (OSError, subprocess.SubprocessError):
                    cleanup_errors.append("invalid-startup container")
            try:
                cleanup = compose("down", "--volumes", "--remove-orphans", check=False, limit=60)
                if cleanup.returncode:
                    cleanup_errors.append("Compose project")
            except (OSError, subprocess.SubprocessError):
                cleanup_errors.append("Compose project")
            if cleanup_errors:
                # A success report must never hide owned resources left behind.
                raise RuntimeError(f"cleanup failed for owned isolated project {project}: {', '.join(cleanup_errors)}")


def write_report(path, report):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix="." + path.name, delete=False) as output:
            temporary = output.name
            json.dump(report, output, ensure_ascii=False, indent=2)
            output.write("\n")
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True, help="already-built local production image")
    parser.add_argument("--timeout", type=int, default=240)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--require-production-source", action="store_true",
                        help="require a clean tracked source revision; omit only for diagnostic builds")
    parser.add_argument("--workflow", action="store_true",
                        help="also test deterministic multi-role review, publish and service credentials")
    parser.add_argument("--browser", action="store_true",
                        help="also run Chromium login/PoW and authenticated five-role UI smoke (requires --workflow)")
    args = parser.parse_args()
    if not args.image or args.image.startswith("-") or not 30 <= args.timeout <= 600:
        parser.error("a valid local image and timeout of 30-600 seconds are required")
    if args.browser and not args.workflow:
        parser.error("--browser requires --workflow")
    if args.output:
        write_report(args.output, {"status": "running", "image": args.image})
    try:
        report = run_smoke(args.image, args.timeout, args.require_production_source, args.workflow, args.browser)
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        if args.output:
            write_report(args.output, {"status": "failed", "image": args.image, "error": str(error)})
        parser.error(str(error))
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        write_report(args.output, report)
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
