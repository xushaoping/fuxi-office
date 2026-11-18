import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class DebianPackageInstallTests(unittest.TestCase):
    def run_installer(self, overrides=None, packages=("curl", "libssl-dev")):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            log = root / "calls"
            for name, body in {
                "timeout": 'printf "timeout %s\\n" "$*" >> "$FIXTURE_LOG"\n'
                           '[ "${FIXTURE_TIMEOUT:-0}" = 0 ] || exit "$FIXTURE_TIMEOUT"\n'
                           'shift 3\nexec "$@"\n',
                "apt-get": 'printf "apt %s\\n" "$*" >> "$FIXTURE_LOG"\n'
                           'if [[ "${!#}" == update ]]; then exit "${FIXTURE_UPDATE:-0}"; fi\n'
                           'exit "${FIXTURE_INSTALL:-0}"\n',
            }.items():
                tool = root / name
                tool.write_text('#!/usr/bin/env bash\nset -eu\n' + body)
                tool.chmod(0o700)
            env = {key: value for key, value in os.environ.items() if not key.startswith(("APT_", "FIXTURE_"))}
            env.update(PATH=str(root) + os.pathsep + env["PATH"], FIXTURE_LOG=str(log), **(overrides or {}))
            result = subprocess.run(["bash", str(ROOT / "script/install-debian-packages.sh"), *packages],
                                    env=env, capture_output=True, text=True, timeout=10)
            return result, log.read_text() if log.exists() else ""

    def test_success_has_bounded_phases_and_requires_complete_index(self):
        result, calls = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("--signal=TERM --kill-after=10s 180s apt-get", calls)
        self.assertIn("--kill-after=10s 600s apt-get", calls)
        self.assertIn("APT::Update::Error-Mode=any update", calls)
        self.assertIn("Acquire::http::Timeout=20", calls)
        self.assertIn("Acquire::Retries=2", calls)
        self.assertIn("install --yes --no-install-recommends curl libssl-dev", calls)

    def test_failed_update_cannot_install_using_old_indexes(self):
        result, calls = self.run_installer({"FIXTURE_UPDATE": "100"})
        self.assertEqual(result.returncode, 100)
        self.assertNotIn(" install ", calls)
        self.assertIn("failed: update (exit 100)", result.stderr)

    def test_download_deadline_is_preserved_and_diagnosed(self):
        result, calls = self.run_installer({"FIXTURE_TIMEOUT": "124", "APT_UPDATE_TIMEOUT": "1"})
        self.assertEqual(result.returncode, 124)
        self.assertIn("1s apt-get", calls)
        self.assertNotIn("\napt ", calls)
        self.assertIn("failed: update (exit 124)", result.stderr)

    def test_install_failure_is_not_hidden(self):
        result, _ = self.run_installer({"FIXTURE_INSTALL": "100"})
        self.assertEqual(result.returncode, 100)
        self.assertIn("failed: install (exit 100)", result.stderr)

    def test_invalid_settings_and_packages_cannot_start_apt(self):
        for overrides, packages in (({"APT_DOWNLOAD_RETRIES": "6"}, ("curl",)),
                                    ({"APT_UPDATE_TIMEOUT": "0"}, ("curl",)),
                                    ({"APT_INSTALL_TIMEOUT": "-1"}, ("curl",)),
                                    ({}, ("--allow-unauthenticated",)), ({}, ())):
            with self.subTest(overrides=overrides, packages=packages):
                result, calls = self.run_installer(overrides, packages)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(calls, "")


if __name__ == "__main__":
    unittest.main()
