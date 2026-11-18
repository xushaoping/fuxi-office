#!/usr/bin/env python3
"""Run a release binary against a newly owned, disposable PostgreSQL cluster."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def load_script(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "script" / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


IMAGE = load_script("smoke-production-image")
VERIFY = load_script("verify-release-artifacts")
WORKFLOW = load_script("production_smoke_workflow")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Client:
    def __init__(self, base):
        self.base = base
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def request(self, path, method="GET", token=None, data=None, expected=200, extra_headers=None):
        if not path.startswith("/") or path.startswith("//"):
            raise ValueError("fixture requests must be relative to the owned loopback server")
        headers = {"Accept": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        headers.update(extra_headers or {})
        if data is not None:
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(self.base + path, method=method, headers=headers,
                                         data=json.dumps(data).encode() if data is not None else None)
        try:
            with self.opener.open(request, timeout=10) as response:
                status, payload = response.status, response.read(16 * 1024 * 1024)
        except urllib.error.HTTPError as error:
            status, payload = error.code, error.read(4096)
        if status != expected:
            raise RuntimeError(f"isolated API {method} {path}: expected {expected}, got {status}")
        return payload

    def eventually(self, path, expected, app, seconds=30):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if app.poll() is not None:
                raise RuntimeError(f"isolated binary exited before {path} (exit {app.returncode})")
            try:
                return self.request(path, expected=expected)
            except (RuntimeError, urllib.error.URLError, TimeoutError):
                time.sleep(0.1)
        raise RuntimeError(f"isolated API timed out checking {path}")


def run_smoke(binary, site, pg_bin_dir=None, browser=False):
    binary, site = binary.resolve(), site.resolve()
    verified = VERIFY.verify_bundle(site, binary, ROOT)
    manifest = json.loads((site / "release-manifest.json").read_text())
    if any(manifest.get(key) != verified[key] for key in ("assetVersion", "binary", "indexSha256", "assets")):
        raise ValueError("release manifest does not match the supplied binary and assets; rebuild the release")
    env = IMAGE.isolated_environment()
    if pg_bin_dir is None:
        pg_bin_dir = subprocess.run(["pg_config", "--bindir"], env=env, capture_output=True,
                                    text=True, timeout=10, check=True).stdout.strip()
    pg = Path(pg_bin_dir).resolve()
    for name in ("initdb", "pg_ctl", "psql"):
        if not (pg / name).is_file():
            raise ValueError(f"PostgreSQL server tool is missing: {name}")
    with tempfile.TemporaryDirectory(prefix="fuxi-binary-smoke-", dir="/tmp") as directory:
        work = Path(directory)
        data = work / "postgres-data"
        pg_port = str(55000 + secrets.randbelow(10000))
        env.update(LC_ALL="C", LANG="C", PGHOST=str(work), PGPORT=pg_port, PGUSER="postgres", PGDATABASE="fuxi_dmn",
                   PATH=str(pg) + os.pathsep + env.get("PATH", ""))
        log_dir = ROOT / "target/test-results"
        log_dir.mkdir(parents=True, exist_ok=True)
        app = None
        cluster_started = False

        def run(command, timeout=30, check=True, environment=None):
            result = subprocess.run(command, cwd=work, env=environment or env,
                                    capture_output=True, text=True, timeout=timeout)
            if check and result.returncode:
                raise RuntimeError(f"isolated fixture operation failed: {Path(command[0]).name} (exit {result.returncode})")
            return result

        def sql(statement):
            return run([str(pg / "psql"), "-X", "-v", "ON_ERROR_STOP=1", "-c", statement])

        try:
            run([str(pg / "initdb"), "-D", str(data), "-A", "trust", "-U", "postgres", "--no-sync", "--encoding=UTF8",
                 "-c", "shared_memory_type=mmap", "-c", "dynamic_shared_memory_type=mmap"])
            # Private Unix socket only; there is no fixture DB TCP listener.
            cluster_started = True  # Also attempt cleanup if pg_ctl itself times out.
            start_database = [str(pg / "pg_ctl"), "-D", str(data), "-l", str(work / "postgres.log"),
                              "-o", f"-p {pg_port} -k {work} -c listen_addresses=''", "-w", "start"]
            run(start_database)
            run([str(pg / "psql"), "-X", "-v", "ON_ERROR_STOP=1", "-d", "postgres", "-c", "CREATE DATABASE fuxi_dmn"])
            bootstrap_env = dict(env, POSTGRES_USER="postgres", POSTGRES_DB="fuxi_dmn",
                                 DB_PASSWORD=secrets.token_hex(24), APP_DB_USER="fuxi", APP_DB_OWNER="fuxi_owner")
            run(["bash", str(ROOT / "script/bootstrap-production-db.sh")], timeout=90, environment=bootstrap_env)
            (work / "target").mkdir()
            (work / "target/site").symlink_to(site, target_is_directory=True)
            with socket.socket() as port_probe:
                port_probe.bind(("127.0.0.1", 0))
                http_port = port_probe.getsockname()[1]
            secret = secrets.token_hex(32)
            app_env = dict(IMAGE.isolated_environment(), APP_ENV="production",
                           BIND_ADDR=f"127.0.0.1:{http_port}", JWT_SECRET=secret,
                           ADMIN_PASSWORD=secrets.token_hex(24), OPENAI_API_KEY="",
                           FUXI_EMBEDDING_ENABLED="false", EMBEDDING_ENABLED="false",
                           OPTIMIZATION_AUTODRAFT_MODE="off", TRUST_PROXY_HEADERS="false",
                           SHUTDOWN_GRACE_SECONDS="10", SHUTDOWN_DRAIN_SECONDS="0",
                           DATABASE_URL=f"postgresql://fuxi@localhost:{pg_port}/fuxi_dmn?host={urllib.parse.quote(str(work), safe='')}")
            with (log_dir / "production-binary-app.log").open("w") as app_log:
                app = subprocess.Popen([str(binary)], cwd=work, env=app_env,
                                       stdout=app_log, stderr=subprocess.STDOUT)
                client = Client(f"http://127.0.0.1:{http_port}")
                client.eventually("/api/ready", 200, app)
                client.request("/api/health")
                served_manifest = json.loads(client.request("/release-manifest.json"))
                if served_manifest != manifest:
                    raise RuntimeError("served release manifest differs from the verified artifact")
                shell = client.request("/").decode()
                version, assets = IMAGE.verified_asset_entries(manifest)
                for path, entry in assets.items():
                    if "/" + path + "?v=" + version not in shell:
                        raise RuntimeError("served shell uses a different release version")
                    payload = client.request("/" + path)
                    if len(payload) != entry["size"] or hashlib.sha256(payload).hexdigest() != entry["sha256"]:
                        raise RuntimeError("served release asset integrity failed")
                token_for = lambda username: IMAGE.fixture_token(username, secret)

                def browser_smoke(project, tokens):
                    if not browser:
                        return None
                    node = shutil.which("node", path=env.get("PATH"))
                    if not node:
                        raise RuntimeError("--browser requires Node.js")
                    browser_env = dict(env,
                                       FUXI_BROWSER_BASE_URL=client.base,
                                       FUXI_BROWSER_TOKENS_JSON=json.dumps(tokens),
                                       FUXI_BROWSER_PROJECT_NAME=project["name"],
                                       FUXI_BROWSER_SCREENSHOT_DIR=str(log_dir))
                    result = subprocess.run([node, str(ROOT / "script/smoke-production-browser.cjs")],
                                            cwd=work, env=browser_env, capture_output=True, text=True,
                                            timeout=120)
                    if result.returncode:
                        (log_dir / "production-browser.log").write_text(
                            result.stdout + result.stderr, encoding="utf-8")
                        raise RuntimeError("isolated browser fixture failed; inspect target/test-results/production-browser.log")
                    return json.loads(result.stdout)

                workflow = WORKFLOW.run_workflow(client.request, token_for,
                                                 browser_smoke if browser else None)
                # An actual DB error must reveal neither column names nor SQL diagnostics.
                sql("ALTER TABLE decision_projects RENAME COLUMN name TO smoke_hidden_name")
                try:
                    error = json.loads(client.request("/api/projects", token=token_for("governance-modeler"), expected=500))
                    if error != {"ok": False, "code": "DATABASE_ERROR", "error": "读取决策项目失败，请稍后重试"}:
                        raise RuntimeError("production API leaked or changed its sanitized database error contract")
                finally:
                    sql("ALTER TABLE decision_projects RENAME COLUMN smoke_hidden_name TO name")
                sql("ALTER TABLE decision_run_explanations RENAME COLUMN generation_revision TO smoke_hidden_revision")
                try:
                    client.eventually("/api/ready", 503, app)
                    client.request("/api/health")
                finally:
                    sql("ALTER TABLE decision_run_explanations RENAME COLUMN smoke_hidden_revision TO generation_revision")
                client.eventually("/api/ready", 200, app)
                run([str(pg / "pg_ctl"), "-D", str(data), "-m", "fast", "-w", "stop"])
                client.eventually("/api/ready", 503, app)
                client.request("/api/health")
                run(start_database)
                client.eventually("/api/ready", 200, app)
                app.terminate()
                if app.wait(timeout=15) != 0:
                    raise RuntimeError("release binary failed graceful SIGTERM")
            invalid = run([str(binary)], environment=dict(IMAGE.isolated_environment(), APP_ENV="production"), check=False)
            if invalid.returncode != 1:
                raise RuntimeError("missing configuration did not produce application exit 1")
            return {"status": "passed", "artifact": "native_release_binary", "assetVersion": version,
                    "binarySha256": verified["binary"]["sha256"], "source": manifest.get("source"),
                    "productionSourceEligible": manifest.get("productionSourceEligible") is True,
                    "workflow": workflow, "externalModelCalls": 0,
                    "checks": ["served_assets", "production_error_redaction", "missing_schema_unready",
                               "database_outage_and_recovery", "sigterm", "invalid_startup"],
                    "browserEnabled": browser,
                    "scope": "owned native release/API deployment; browser mode covers login PoW rejection and authenticated project pages, not Docker, provider, load or business-quality acceptance"}
        finally:
            if app is not None and app.poll() is None:
                app.terminate()
                try:
                    app.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait(timeout=5)
            if cluster_started:
                stopped = run([str(pg / "pg_ctl"), "-D", str(data), "-m", "immediate", "-w", "stop"], check=False)
                # The cluster may already be stopped by a preceding failed probe.
                if stopped.returncode and (data / "postmaster.pid").exists():
                    raise RuntimeError("failed to stop owned fixture PostgreSQL cluster")
            if (work / "postgres.log").exists():
                shutil.copyfile(work / "postgres.log", log_dir / "production-binary-postgres.log")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/fuxi-dmn")
    parser.add_argument("--site-root", type=Path, default=ROOT / "target/site")
    parser.add_argument("--postgres-bin", type=Path)
    parser.add_argument("--browser", action="store_true",
                        help="run Chromium login/PoW and authenticated five-role UI smoke")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.output:
        VERIFY.write_manifest(args.output, {"status": "running", "artifact": "native_release_binary"})
    try:
        report = run_smoke(args.binary, args.site_root, args.postgres_bin, args.browser)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        if args.output:
            VERIFY.write_manifest(args.output, {"status": "failed", "artifact": "native_release_binary",
                                               "error": str(error)})
        parser.error(str(error))
    if args.output:
        VERIFY.write_manifest(args.output, report)
    else:
        print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
