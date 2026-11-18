import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).parents[1] / "script/check-release-candidate.py"
SPEC = importlib.util.spec_from_file_location("release_candidate", SCRIPT)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class ReleaseCandidateTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.cargo_version = "0.2.0"
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "candidate-fixture"\nversion = "0.2.0"\nedition = "2021"\n')
        (self.root / "src").mkdir()
        (self.root / "src/main.rs").write_text("fn main() {}\n")
        self.git("init", "-q")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "user.name", "Release Fixture")
        self.git("add", "Cargo.toml", "src/main.rs")
        self.git("commit", "-qm", "candidate fixture")
        self.git("tag", "v0.1.2")

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True,
                              capture_output=True, text=True)

    def check(self, version, require_tag=False):
        with patch.object(CHECK, "package_version", return_value=self.cargo_version):
            return CHECK.check_candidate(self.root, version, require_tag)

    def test_accepts_clean_version_above_latest_stable_tag(self):
        report = self.check("0.2.0")
        self.assertEqual(report["status"], "passed")
        self.assertEqual(report["latestStableTag"], "v0.1.2")
        self.assertTrue(report["productionSourceEligible"])
        self.assertFalse(report["tagVerified"])

    def test_rejects_version_mismatch_and_non_increasing_release(self):
        with self.assertRaisesRegex(ValueError, "does not match"):
            self.check("0.3.0")
        self.cargo_version = "0.1.2"
        (self.root / "Cargo.toml").write_text(
            '[package]\nname = "candidate-fixture"\nversion = "0.1.2"\nedition = "2021"\n')
        self.git("add", "Cargo.toml")
        self.git("commit", "-qm", "old version")
        with self.assertRaisesRegex(ValueError, "higher than"):
            self.check("0.1.2")

    def test_rejects_dirty_and_untracked_worktrees(self):
        (self.root / "untracked.txt").write_text("not committed")
        with self.assertRaisesRegex(ValueError, "worktree must be clean"):
            self.check("0.2.0")

    def test_required_tag_must_point_to_candidate_commit(self):
        self.git("tag", "v0.2.0")
        report = self.check("0.2.0", require_tag=True)
        self.assertTrue(report["tagVerified"])
        (self.root / "src/main.rs").write_text("fn main() { let _ = 1; }\n")
        self.git("add", "src/main.rs")
        self.git("commit", "-qm", "candidate changed after tag")
        self.git("tag", "-f", "v0.2.0", "v0.1.2")
        with self.assertRaisesRegex(ValueError, "does not point to HEAD"):
            self.check("0.2.0", require_tag=True)

    def test_rejects_invalid_release_version(self):
        with self.assertRaisesRegex(ValueError, "MAJOR.MINOR.PATCH"):
            self.check("v0.2.0")

    def test_reads_real_root_package_version_with_cargo_metadata(self):
        self.assertEqual(CHECK.package_version(Path(__file__).parents[1]), "0.1.0")


if __name__ == "__main__":
    unittest.main()
