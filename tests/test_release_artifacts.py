import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "release_artifacts", Path(__file__).parents[1] / "script/verify-release-artifacts.py")
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


class ReleaseArtifactTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.site = self.root / "site"
        (self.site / "pkg").mkdir(parents=True)
        (self.site / "pkg/fuxi-dmn.wasm").write_bytes(b"\x00asm\x01\x00\x00\x00fixture")
        (self.site / "pkg/fuxi-dmn.js").write_text("export function hydrate() {}")
        (self.site / "pkg/fuxi-dmn.css").write_text("body {}")
        self.binary = self.root / "fuxi-dmn"
        self.binary.write_bytes(b"fixture executable")
        self.binary.chmod(0o700)
        self.stamp_shell()

    def stamp_shell(self):
        _, self.version = VERIFY.asset_inventory(self.site)
        (self.site / "index.html").write_text("\n".join(
            f'"/{name}?v={self.version}"' for name in VERIFY.REQUIRED_ASSETS))

    def test_manifest_verifies_bundle_and_binds_all_static_files(self):
        (self.site / "images").mkdir()
        (self.site / "images/logo.png").write_bytes(b"fixture")
        self.stamp_shell()
        report = VERIFY.verify_bundle(self.site, self.binary, self.root)
        self.assertEqual(report["assetVersion"], self.version)
        self.assertEqual(len(report["assets"]), 4)
        self.assertFalse(report["productionSourceEligible"])
        VERIFY.write_manifest(self.site / "release-manifest.json", report)
        self.assertEqual(VERIFY.asset_inventory(self.site)[1], self.version)

    def test_mixed_release_assets_are_rejected(self):
        (self.site / "pkg/fuxi-dmn.js").write_text("new release")
        with self.assertRaisesRegex(ValueError, "current asset digest"):
            VERIFY.verify_bundle(self.site, self.binary, self.root)

    def test_missing_wasm_and_corrupted_wasm_are_rejected(self):
        wasm = self.site / "pkg/fuxi-dmn.wasm"
        wasm.write_bytes(b"not a WASM file")
        with self.assertRaisesRegex(ValueError, "invalid header"):
            VERIFY.asset_inventory(self.site)
        wasm.unlink()
        with self.assertRaisesRegex(ValueError, "missing or empty"):
            VERIFY.asset_inventory(self.site)

    def test_symlink_cannot_escape_the_bundle(self):
        (self.site / "outside").symlink_to(self.binary)
        with self.assertRaisesRegex(ValueError, "symbolic"):
            VERIFY.asset_inventory(self.site)

    def test_unresolved_shell_and_missing_binary_are_rejected(self):
        (self.site / "index.html").write_text("__FUXI_ASSET_VERSION__")
        with self.assertRaisesRegex(ValueError, "unresolved"):
            VERIFY.verify_bundle(self.site, self.binary, self.root)
        self.stamp_shell()
        self.binary.unlink()
        with self.assertRaisesRegex(ValueError, "executable"):
            VERIFY.verify_bundle(self.site, self.binary, self.root)

    def test_production_candidate_requires_source_provenance(self):
        with self.assertRaisesRegex(ValueError, "clean, tracked"):
            VERIFY.verify_bundle(self.site, self.binary, self.root, require_clean=True)

    def test_container_provenance_requires_a_valid_commit_and_clean_pipeline_input(self):
        with patch.dict(VERIFY.os.environ, {"FUXI_BUILD_COMMIT": "a" * 40,
                                            "FUXI_SOURCE_CLEAN": "1"}, clear=True):
            report = VERIFY.verify_bundle(self.site, self.binary, self.root, require_clean=True)
            self.assertTrue(report["productionSourceEligible"])
            self.assertEqual(report["source"]["source"], "build_pipeline_metadata")
        for supplied in ("invalid", "a" * 40):
            with patch.dict(VERIFY.os.environ, {"FUXI_BUILD_COMMIT": supplied,
                                                "FUXI_SOURCE_CLEAN": "0"}, clear=True):
                with self.assertRaisesRegex(ValueError, "clean, tracked"):
                    VERIFY.verify_bundle(self.site, self.binary, self.root, require_clean=True)


if __name__ == "__main__":
    unittest.main()
