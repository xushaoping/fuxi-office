"""Exercise the deployment helper without connecting to PostgreSQL or Docker."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import urllib.parse


SOURCE = Path(__file__).resolve().parents[1] / "script" / "upgrade-decision-explanations.sh"
MIGRATIONS = [
    "092_decision_run_explanations.sql",
    "094_decision_run_explanation_reviews.sql",
    "095_prompt_registry_tester_judge.sql",
    "096_decision_explanation_attempt_ownership.sql",
    "097_decision_explanation_review_revisions.sql",
]
FAKE_TOOL = r'''
import json
import os
from pathlib import Path
import sys

body = sys.stdin.read()
entry = {"tool": Path(sys.argv[0]).name, "args": sys.argv[1:], "body": body,
         "database": os.environ.get("PGDATABASE")}
with open(os.environ["FAKE_CALL_LOG"], "a", encoding="utf-8") as output:
    output.write(json.dumps(entry) + "\n")
if "BEGIN READ ONLY;" in body:
    installed = "check_installed=1" in sys.argv
    if os.environ.get("FAKE_MISSING") == "base" or (
        installed and os.environ.get("FAKE_MISSING") == "installed"
    ):
        print("missing prerequisite table: prompt_assets" if not installed
              else "missing installed column: decision_run_explanations.generation_revision")
else:
    if os.environ.get("FAKE_FAIL_MIGRATION", "not-a-migration") in body:
        print("fixture migration failure", file=sys.stderr)
        sys.exit(7)
'''


class UpgradeDecisionExplanationsTests(unittest.TestCase):
    def setUp(self):
        self.actual_psql = shutil.which("psql")
        self.temporary = tempfile.TemporaryDirectory(prefix="explanation-upgrade-test-")
        self.addCleanup(self.temporary.cleanup)
        # These names would execute commands or split words under unsafe expansion.
        self.root = Path(self.temporary.name) / "repo space 'quote $literal `backticks`"
        (self.root / "script").mkdir(parents=True)
        self.script = self.root / "script" / SOURCE.name
        shutil.copyfile(SOURCE, self.script)
        self.script.chmod(0o700)
        for name in MIGRATIONS:
            (self.root / "script" / name).write_text(f"-- fixture {name}\nSELECT 1;\n")
        (self.root / "docker-compose.yml").write_text("services: {}\n")
        self.env_file = self.root / ".env space $literal 'quote"
        self.env_file.write_text("DB_PASSWORD=fixture\n")
        self.log = Path(self.temporary.name) / "calls.jsonl"
        self.bin = Path(self.temporary.name) / "bin"
        self.bin.mkdir()
        for name in ["psql", "docker"]:
            tool = self.bin / name
            tool.write_text(f"#!{sys.executable}\n" + FAKE_TOOL)
            tool.chmod(0o700)
        self.environment = os.environ.copy()
        self.environment.update({
            "PATH": str(self.bin) + os.pathsep + self.environment["PATH"],
            "FAKE_CALL_LOG": str(self.log),
            "DATABASE_URL": "postgresql://fixture:do-not-log-secret@localhost/example",
            "APP_DB_USER": "application $role 'quoted",
        })
        for name in ["FAKE_MISSING", "FAKE_FAIL_MIGRATION", "POSTGRES_USER", "POSTGRES_DB"]:
            self.environment.pop(name, None)

    def run_script(self, *arguments, overrides=None):
        environment = self.environment.copy()
        if overrides:
            for key, value in overrides.items():
                if value is None:
                    environment.pop(key, None)
                else:
                    environment[key] = value
        return subprocess.run(
            ["bash", str(self.script), *arguments],
            env=environment, text=True, capture_output=True, check=False,
        )

    def calls(self):
        if not self.log.exists():
            return []
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_applies_order_and_checks_before_and_after_without_printing_credentials(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertEqual(len(calls), len(MIGRATIONS) + 2)
        self.assertIn("BEGIN READ ONLY;", calls[0]["body"])
        self.assertIn("check_installed=0", calls[0]["args"])
        self.assertIn("check_installed=1", calls[-1]["args"])
        for call, expected in zip(calls[1:-1], MIGRATIONS):
            self.assertIn(expected, call["body"])
            self.assertIn("app_user=application $role 'quoted", call["args"])
            self.assertIn("--dbname=" + self.environment["DATABASE_URL"], call["args"])
        self.assertIn("restart the application", result.stdout)
        self.assertNotIn("do-not-log-secret", result.stdout + result.stderr)

    def test_real_libpq_reads_nondefault_uri_port_instead_of_default_socket(self):
        if not self.actual_psql:
            self.skipTest("requires local psql for actual libpq URI parsing")
        # A missing private socket exercises the real URI parser without a
        # running server, remote connection, database read, or migration write.
        socket_dir = Path(self.temporary.name) / "missing-socket"
        uri = "postgresql://localhost:65431/example?host=" + urllib.parse.quote(str(socket_dir), safe="")
        environment = self.environment.copy()
        environment.update({
            "PATH": os.environ["PATH"],
            "DATABASE_URL": uri,
            "PGPORT": "5432",
        })
        environment.pop("PGDATABASE", None)
        result = subprocess.run(
            ["bash", str(self.script), "--check"], env=environment,
            text=True, capture_output=True, check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(".s.PGSQL.65431", result.stderr)
        self.assertNotIn(".s.PGSQL.5432", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_dry_run_does_not_connect_or_require_credentials(self):
        result = self.run_script("--dry-run", overrides={"DATABASE_URL": None, "APP_DB_USER": None})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), [])
        for migration in MIGRATIONS:
            self.assertIn(migration, result.stdout)
        self.assertNotIn("are ready", result.stdout)

    def test_check_is_read_only_and_does_not_apply_migrations(self):
        result = self.run_script("--check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls()), 1)
        self.assertIn("BEGIN READ ONLY;", self.calls()[0]["body"])
        self.assertIn("check_installed=1", self.calls()[0]["args"])
        self.assertNotIn("applying", result.stdout)

    def test_missing_prerequisites_prevent_all_migration_writes(self):
        result = self.run_script(overrides={"FAKE_MISSING": "base"})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(self.calls()), 1)
        self.assertIn("upgrade-prompt-registry.sh", result.stderr)
        self.assertNotIn("applying", result.stdout)

    def test_failed_migration_stops_later_migrations_and_success_message(self):
        failed = MIGRATIONS[2]
        result = self.run_script(overrides={"FAKE_FAIL_MIGRATION": failed})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(self.calls()), 4)  # preflight and migrations through 095
        self.assertNotIn(MIGRATIONS[3], result.stdout)
        self.assertNotIn("are ready", result.stdout)

    def test_failed_final_schema_check_does_not_claim_success(self):
        result = self.run_script(overrides={"FAKE_MISSING": "installed"})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(self.calls()), len(MIGRATIONS) + 2)
        self.assertIn("missing installed column", result.stderr)
        self.assertNotIn("are ready", result.stdout)

    def test_docker_streams_host_files_and_preserves_special_paths_and_roles(self):
        result = self.run_script("--docker-compose", "--env-file", str(self.env_file), overrides={
            "APP_DB_USER": None, "POSTGRES_USER": "migration $role 'quoted", "POSTGRES_DB": "db space",
            "DATABASE_URL": None,
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        self.assertTrue(all(call["tool"] == "docker" for call in calls))
        self.assertIn(str(self.env_file), calls[0]["args"])
        self.assertIn(str(self.root / "docker-compose.yml"), calls[0]["args"])
        self.assertIn("migration $role 'quoted", calls[0]["args"])
        self.assertIn("db space", calls[0]["args"])
        self.assertIn("app_user=migration $role 'quoted", calls[0]["args"])
        for call, migration in zip(calls[1:-1], MIGRATIONS):
            self.assertIn(migration, call["body"])
            self.assertEqual(call["args"][-2:], ["-f", "-"])

    def test_bad_arguments_or_missing_files_never_connect(self):
        for arguments in [
            ["--unknown"], ["--env-file"], ["--env-file", "a"],
            ["--dry-run", "--check"], ["--dry-run", "--dry-run"],
            ["--docker-compose", "--docker-compose"],
            ["--docker-compose", "--env-file", "--check"],
        ]:
            with self.subTest(arguments=arguments):
                result = self.run_script(*arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.calls(), [])
        (self.root / "script" / MIGRATIONS[-1]).unlink()
        result = self.run_script()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing migration file", result.stderr)
        self.assertEqual(self.calls(), [])

    def test_missing_connection_or_app_role_never_connects(self):
        for overrides in [{"DATABASE_URL": None}, {"APP_DB_USER": None}]:
            with self.subTest(overrides=overrides):
                result = self.run_script(overrides=overrides)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
