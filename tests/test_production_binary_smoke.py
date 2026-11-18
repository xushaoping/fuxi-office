"""Failure behavior only; actual startup and role coverage use the owned PG runner."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('native_smoke', ROOT / 'script/smoke-production-binary.py')
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)


class BinarySmokeFailureTests(unittest.TestCase):
    def test_missing_release_cannot_start_a_database_or_service(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(NATIVE.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'site directory is missing'):
                NATIVE.run_smoke(Path(directory) / 'binary', Path(directory) / 'site')
            run.assert_not_called()

    def test_mismatched_manifest_cannot_start_processes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / 'release-manifest.json'
            manifest.write_text(json.dumps({'assetVersion': 'old'}))
            verified = {'assetVersion': 'new', 'binary': {}, 'indexSha256': 'hash', 'assets': []}
            with mock.patch.object(NATIVE.VERIFY, 'verify_bundle', return_value=verified), \
                 mock.patch.object(NATIVE.subprocess, 'run') as run:
                with self.assertRaisesRegex(ValueError, 'does not match'):
                    NATIVE.run_smoke(root / 'binary', root)
                run.assert_not_called()

    def test_failed_attempt_replaces_a_stale_pass_report(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / 'result.json'
            report.write_text('{"status":"passed"}')
            args = ['smoke-production-binary.py', '--output', str(report)]
            with mock.patch('sys.argv', args), mock.patch.object(NATIVE, 'run_smoke', side_effect=RuntimeError('fixture failed')), \
                 contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit):
                    NATIVE.main()
            self.assertEqual(json.loads(report.read_text())['status'], 'failed')

    def test_http_client_cannot_follow_redirects_or_absolute_urls(self):
        client = NATIVE.Client('http://127.0.0.1:49152')
        with mock.patch.object(client.opener, 'open') as open_request:
            for path in ('https://outside.invalid/api', '//outside.invalid/api'):
                with self.assertRaises(ValueError):
                    client.request(path)
            open_request.assert_not_called()
        self.assertIsNone(NATIVE.NoRedirect().redirect_request(None, None, 302, '', {}, 'https://outside.invalid'))


if __name__ == '__main__':
    unittest.main()
