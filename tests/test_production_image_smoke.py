"""Offline stateful Docker/HTTP fixtures; these tests never contact a Docker daemon."""
import base64
import copy
import contextlib
import hashlib
import hmac
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest
from unittest import mock
import urllib.error
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('production_image_smoke', ROOT / 'script/smoke-production-image.py')
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)
IMAGE_ID = 'sha256:' + '9a' * 32
CONTAINER_ID = 'c7' * 32
ASSETS = {'pkg/fuxi-dmn.js': b'export default "fixture";',
          'pkg/fuxi-dmn.wasm': b'\0asm\1\0\0\0fixture',
          'pkg/fuxi-dmn.css': b'body { color: #123; }'}
VERSION = 'a1' * 32
MANIFEST = {'assetVersion': VERSION, 'assets': [
    {'path': name, 'sha256': hashlib.sha256(data).hexdigest(), 'size': len(data)}
    for name, data in ASSETS.items()]}


class FixtureResponse:
    def __init__(self, payload, status=200):
        self.status, self.payload = status, payload

    def read(self, limit):
        return self.payload[:limit]

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False


class DeploymentFixture:
    def __init__(self):
        self.commands = []
        self.requests = []
        self.schema_missing = False
        self.database_up = True
        self.manifest = copy.deepcopy(MANIFEST)
        self.shell = '\n'.join('/' + name + '?v=' + VERSION for name in ASSETS)
        self.assets = dict(ASSETS)
        self.local_missing = False
        self.cleanup_failure = False
        self.standalone_cleanup_failure = False
        self.up_failure = False
        self.up_timeout = False
        self.invalid_startup_code = 1
        self.app_exit = '0'
        self.port = '127.0.0.1:38491'
        self.project = None
        self.fixture_env = None
        self.env_path = None
        self.down_called = False
        self.invalid_removed = False
        self.fixture_file_mode = None
        self.fail_path = None
        self.ready_stuck = False
        self.clock = 0.0
        self.bad_metadata = None

    def tick(self):
        self.clock += 1.0
        return self.clock

    @staticmethod
    def result(command, code=0, stdout=''):
        return subprocess.CompletedProcess(command, code, stdout=stdout, stderr='fixture error' if code else '')

    def docker(self, command, **kwargs):
        self.commands.append((list(command), dict(kwargs)))
        if command[:3] == ['docker', 'image', 'inspect']:
            if self.local_missing:
                return self.result(command, 1)
            metadata = self.bad_metadata if self.bad_metadata is not None else {
                'Id': IMAGE_ID, 'Os': 'linux', 'Architecture': 'arm64'}
            return self.result(command, stdout=json.dumps(metadata))
        if command[:2] == ['docker', 'compose']:
            self.project = command[command.index('--project-name') + 1]
            self.env_path = Path(command[command.index('--env-file') + 1])
            self.fixture_file_mode = stat.S_IMODE(self.env_path.stat().st_mode)
            self.fixture_env = dict(line.split('=', 1) for line in self.env_path.read_text().splitlines())
            action = command[command.index('-f') + 2:]
            if action[0] == 'up':
                if self.up_timeout:
                    raise subprocess.TimeoutExpired(command, kwargs['timeout'])
                return self.result(command, 1 if self.up_failure else 0)
            if action[0] == 'port':
                return self.result(command, stdout=self.port + '\n')
            if action[:3] == ['ps', '-q', 'app']:
                return self.result(command, stdout=CONTAINER_ID + '\n')
            if action[:2] == ['stop', 'postgres']:
                self.database_up = False
            if action[:2] == ['start', 'postgres']:
                self.database_up = True
            if action[0] == 'exec':
                if 'TO smoke_hidden_revision' in action[-1]:
                    self.schema_missing = True
                elif 'TO generation_revision' in action[-1]:
                    self.schema_missing = False
                else:
                    raise AssertionError('unexpected SQL in fixture')
            if action[0] == 'down':
                self.down_called = True
                self.assert_owned_down(action)
                return self.result(command, 1 if self.cleanup_failure else 0)
            return self.result(command)
        if command[:2] == ['docker', 'inspect']:
            assert command[-1] == CONTAINER_ID
            return self.result(command, stdout=self.app_exit + '\n')
        if command[:2] == ['docker', 'run']:
            assert command[-1] == IMAGE_ID
            assert '--pull=never' in command
            assert command[command.index('--network') + 1] == 'none'
            assert command[command.index('--name') + 1] == self.project + '-invalid-startup'
            return self.result(command, self.invalid_startup_code)
        if command[:3] == ['docker', 'rm', '--force']:
            assert command[-1] == self.project + '-invalid-startup'
            self.invalid_removed = True
            return self.result(command, 1 if self.standalone_cleanup_failure else 0)
        raise AssertionError('unexpected subprocess: ' + str(command))

    @staticmethod
    def assert_owned_down(action):
        assert action == ['down', '--volumes', '--remove-orphans']

    def open(self, req, timeout):
        self.requests.append(req)
        url = urlsplit(req.full_url)
        assert url.scheme == 'http' and url.hostname == '127.0.0.1' and url.port == 38491
        path, method = url.path, req.get_method()
        payload = json.loads(req.data) if req.data else None
        status, body = 200, {}
        token = req.headers.get('Authorization')
        username = None
        if token:
            parts = token.split(' ')[1].split('.')
            signed = '.'.join(parts[:2]).encode()
            expected = base64.urlsafe_b64encode(hmac.new(self.fixture_env['JWT_SECRET'].encode(), signed,
                                                       hashlib.sha256).digest()).decode().rstrip('=')
            assert parts[2] == expected, 'JWT must use this deployment fixture secret'
            username = json.loads(base64.urlsafe_b64decode(parts[1] + '==='))['sub']
        if path == self.fail_path:
            status = 500
        elif path == '/api/ready':
            status = 503 if self.schema_missing or not self.database_up or self.ready_stuck else 200
        elif path == '/api/health':
            pass
        elif path == '/release-manifest.json':
            body = self.manifest
        elif path == '/':
            body = self.shell.encode()
        elif path[1:] in self.assets:
            body = self.assets[path[1:]]
        elif path == '/api/me':
            assert username == 'admin'
        elif path == '/api/admin/users' and method == 'POST':
            assert username == 'admin' and payload['role'] == 'user'
            assert payload['username'] in {'smoke-viewer', 'smoke-outsider'}
            status = 201
        elif path == '/api/projects' and method == 'POST':
            assert username == 'admin' and set(payload) == {'name', 'description', 'riskLevel'}
            assert payload['name'] and payload['riskLevel'] == 'low'
            status, body = 201, {'id': 17}
        elif path == '/api/projects/17/members/smoke-viewer' and method == 'PUT':
            assert username == 'admin' and payload == {'role': 'viewer'}
        elif path == '/api/projects/17/deviation-analysis-inventory':
            assert username in {'smoke-viewer', 'smoke-outsider'}
            status = 200 if username == 'smoke-viewer' else 404
        elif path == '/api/projects/17' and method == 'PUT':
            assert username == 'smoke-viewer'
            # Mirror the real extractor's required name; an empty payload is 422.
            status = 404 if payload.get('name') and payload.get('riskLevel') == 'low' else 422
        else:
            raise AssertionError('unexpected endpoint: ' + method + ' ' + path)
        raw = body if isinstance(body, bytes) else json.dumps(body).encode()
        if status >= 400:
            raise urllib.error.HTTPError(req.full_url, status, 'fixture', {}, io.BytesIO(raw))
        return FixtureResponse(raw, status)


class ProductionImageSmokeTests(unittest.TestCase):
    def setUp(self):
        self.fixture = DeploymentFixture()
        self.run_patch = mock.patch.object(SMOKE.subprocess, 'run', side_effect=self.fixture.docker)
        self.run_patch.start()
        self.addCleanup(self.run_patch.stop)
        self.opener_patch = mock.patch.object(SMOKE.urllib.request, 'build_opener', return_value=self.fixture)
        self.opener = self.opener_patch.start()
        self.addCleanup(self.opener_patch.stop)
        sleep_patch = mock.patch.object(SMOKE.time, 'sleep')
        sleep_patch.start()
        self.addCleanup(sleep_patch.stop)
        clock_patch = mock.patch.object(SMOKE.time, 'monotonic', side_effect=self.fixture.tick)
        clock_patch.start()
        self.addCleanup(clock_patch.stop)

    def run_smoke(self):
        return SMOKE.run_smoke('fixture-release:current', timeout=30)

    def test_full_chain_isolated_immutable_and_cleaned(self):
        contaminated = {
            'APP_DB_OWNER': 'production_owner', 'APP_DB_USER': 'production_superuser',
            'APP_PLATFORM': 'linux/amd64', 'OPENAI_API_KEY': 'do-not-use',
            'OPENAI_MAX_PROJECT_DAILY_TOKENS': '123', 'DATABASE_URL': 'postgres://production',
            'COMPOSE_FILE': '/business.yml', 'COMPOSE_PROFILES': 'production',
            'COMPOSE_PROJECT_NAME': 'business', 'COMPOSE_ENV_FILES': '/production.env',
            'DOCKER_HOST': 'tcp://production:2375', 'DOCKER_CONTEXT': 'business',
            'AGENT_ORCHESTRATION_STALE_AFTER_SECONDS': '999', 'SHUTDOWN_GRACE_SECONDS': '999',
            'REQUIRE_HISTORICAL_REGRESSION_BEFORE_PUBLISH': 'false',
            'DRIFT_WEBHOOK_SIGNING_SECRET': 'production', 'OPTIMIZATION_AUTODRAFT_MODE': 'draft',
            'FUXI_IMAGE': 'production:latest', 'HTTPS_PROXY': 'http://production-proxy',
        }
        with mock.patch.dict(os.environ, contaminated):
            report = self.run_smoke()
        self.assertEqual(report['status'], 'passed')
        self.assertEqual(report['imageId'], IMAGE_ID)
        self.assertEqual(report['platform'], 'linux/arm64')
        self.assertFalse(report['productionSourceEligible'])
        self.assertIsNone(report['sourceCommit'])
        self.assertEqual(report['externalModelCalls'], 0)
        self.assertTrue(self.fixture.down_called)
        self.assertTrue(self.fixture.invalid_removed)
        self.assertFalse(self.fixture.env_path.exists())
        self.assertEqual(self.fixture.fixture_file_mode, 0o600)
        env = self.fixture.fixture_env
        self.assertEqual(env['APP_DB_OWNER'], 'fuxi_owner')
        self.assertEqual(env['APP_DB_USER'], 'fuxi')
        self.assertEqual(env['APP_PLATFORM'], 'linux/arm64')
        self.assertEqual(env['FUXI_IMAGE'], IMAGE_ID)
        self.assertEqual(env['APP_PORT'], '0')
        self.assertEqual(env['OPENAI_API_KEY'], '')
        self.assertEqual(env['FUXI_EMBEDDING_ENABLED'], 'false')
        self.assertEqual(env['OPTIMIZATION_AUTODRAFT_MODE'], 'off')
        for command, kwargs in self.fixture.commands:
            self.assertTrue(set(kwargs['env']).isdisjoint(contaminated), command)
            self.assertIn('timeout', kwargs)
        up = next(command for command, _ in self.fixture.commands if 'up' in command)
        self.assertIn('--no-build', up)
        self.assertEqual(up[up.index('--pull') + 1], 'never')
        self.assertTrue(self.fixture.project.startswith('fuxi-release-smoke-'))
        viewer_update = [req for req in self.fixture.requests if req.get_method() == 'PUT'
                         and req.full_url.endswith('/api/projects/17')]
        self.assertEqual(len(viewer_update), 1)
        self.assertTrue(json.loads(viewer_update[0].data)['name'])
        handlers = self.opener.call_args[0]
        self.assertEqual(handlers[0].proxies, {})
        self.assertIsNone(handlers[1].redirect_request(None, None, 302, '', {}, 'https://external.invalid'))

    def test_local_image_absent_fails_before_resource_creation(self):
        self.fixture.local_missing = True
        with self.assertRaisesRegex(RuntimeError, 'not available locally'):
            self.run_smoke()
        self.assertEqual(len(self.fixture.commands), 1)
        self.assertFalse(self.fixture.down_called)
        self.assertFalse(self.fixture.requests)

    def test_invalid_image_metadata_fails_before_compose(self):
        for metadata in ({'Id': 'invalid', 'Architecture': 'amd64', 'Os': 'linux'},
                         {'Id': IMAGE_ID, 'Architecture': 'arm64', 'Os': 'windows'}, []):
            with self.subTest(metadata=metadata):
                self.fixture.bad_metadata = metadata
                with self.assertRaisesRegex(RuntimeError, 'metadata is invalid'):
                    self.run_smoke()
                self.assertFalse(self.fixture.down_called)

    def test_partial_startup_failure_still_cleans_owned_project(self):
        self.fixture.up_failure = True
        with self.assertRaisesRegex(RuntimeError, 'Compose operation failed: up'):
            self.run_smoke()
        self.assertTrue(self.fixture.down_called)
        self.assertFalse(self.fixture.env_path.exists())

    def test_startup_timeout_still_cleans_owned_project(self):
        self.fixture.up_timeout = True
        with self.assertRaises(subprocess.TimeoutExpired):
            self.run_smoke()
        self.assertTrue(self.fixture.down_called)
        self.assertFalse(self.fixture.env_path.exists())

    def test_http_failure_still_cleans_and_does_not_call_models(self):
        self.fixture.fail_path = '/api/me'
        with self.assertRaisesRegex(RuntimeError, 'expected 200, got 500'):
            self.run_smoke()
        self.assertTrue(self.fixture.down_called)
        self.assertFalse(self.fixture.invalid_removed)

    def test_non_loopback_port_mapping_rejected(self):
        for port in ('0.0.0.0:38491', '127.0.0.1:0', '127.0.0.1:70000', 'bad'):
            with self.subTest(port=port):
                self.fixture.port = port
                with self.assertRaisesRegex(RuntimeError, 'loopback application port'):
                    self.run_smoke()
                self.assertTrue(self.fixture.down_called)
                self.assertFalse(self.fixture.requests)

    def test_ready_timeout_cleans(self):
        self.fixture.ready_stuck = True
        with self.assertRaisesRegex(RuntimeError, 'timed out checking /api/ready'):
            self.run_smoke()
        self.assertTrue(self.fixture.down_called)

    def test_missing_required_asset_never_passes(self):
        for name in ASSETS:
            with self.subTest(name=name):
                self.fixture.manifest = copy.deepcopy(MANIFEST)
                self.fixture.manifest['assets'] = [entry for entry in self.fixture.manifest['assets']
                                                   if entry['path'] != name]
                with self.assertRaisesRegex(RuntimeError, 'all three required'):
                    self.run_smoke()
                self.assertTrue(self.fixture.down_called)

    def test_tampered_asset_or_shell_never_passes(self):
        self.fixture.assets['pkg/fuxi-dmn.wasm'] = b'tampered'
        with self.assertRaisesRegex(RuntimeError, 'digest/size check'):
            self.run_smoke()
        self.assertTrue(self.fixture.down_called)
        self.fixture.assets = dict(ASSETS)
        self.fixture.shell = '/pkg/fuxi-dmn.js?v=old'
        with self.assertRaisesRegex(RuntimeError, 'different versions'):
            self.run_smoke()

    def test_failed_sigterm_is_not_success(self):
        self.fixture.app_exit = '137'
        with self.assertRaisesRegex(RuntimeError, 'did not exit cleanly'):
            self.run_smoke()
        self.assertTrue(self.fixture.down_called)
        self.assertFalse(self.fixture.invalid_removed)

    def test_docker_error_or_startup_success_does_not_count_as_rejection(self):
        for code in (0, 125, 126, 127):
            with self.subTest(code=code):
                self.fixture.invalid_startup_code = code
                with self.assertRaisesRegex(RuntimeError, 'application exit 1'):
                    self.run_smoke()
                self.assertTrue(self.fixture.down_called)
                self.assertTrue(self.fixture.invalid_removed)

    def test_cleanup_failure_overrides_a_passing_report(self):
        for standalone in (False, True):
            with self.subTest(standalone=standalone):
                self.fixture.cleanup_failure = not standalone
                self.fixture.standalone_cleanup_failure = standalone
                with self.assertRaisesRegex(RuntimeError, 'cleanup failed for owned isolated project'):
                    self.run_smoke()
                self.assertTrue(self.fixture.down_called)
                self.assertTrue(self.fixture.invalid_removed)

    def test_cli_production_gate_rejects_dirty_manifest_and_cleans(self):
        self.fixture.manifest['source'] = {'commit': 'e1' * 20, 'workingTreeClean': False}
        self.fixture.manifest['productionSourceEligible'] = False
        args = ['smoke-production-image.py', '--image', 'fixture-release:current',
                '--timeout', '30', '--require-production-source']
        output = io.StringIO()
        with mock.patch.object(SMOKE.sys, 'argv', args), contextlib.redirect_stderr(output):
            with self.assertRaises(SystemExit) as error:
                SMOKE.main()
        self.assertEqual(error.exception.code, 2)
        self.assertIn('clean, tracked source revision', output.getvalue())
        self.assertTrue(self.fixture.down_called)

    def test_clean_production_source_is_reported(self):
        self.fixture.manifest['source'] = {'commit': 'e1' * 20, 'workingTreeClean': True}
        self.fixture.manifest['productionSourceEligible'] = True
        report = SMOKE.run_smoke('fixture-release:current', timeout=30, require_production_source=True)
        self.assertEqual(report['sourceCommit'], 'e1' * 20)
        self.assertTrue(report['productionSourceEligible'])

    def test_invalid_parameters_do_not_execute_docker(self):
        for image, timeout in (('', 30), ('--bad', 30), ('image', 0), ('image', 900)):
            with self.subTest(image=image, timeout=timeout):
                with self.assertRaises(ValueError):
                    SMOKE.run_smoke(image, timeout)
        self.assertEqual(self.fixture.commands, [])

    def test_browser_cli_requires_the_deterministic_role_workflow(self):
        args = ['smoke-production-image.py', '--image', 'fixture-release:current', '--browser']
        output = io.StringIO()
        with mock.patch.object(SMOKE.sys, 'argv', args), contextlib.redirect_stderr(output):
            with self.assertRaises(SystemExit) as error:
                SMOKE.main()
        self.assertEqual(error.exception.code, 2)
        self.assertIn('--browser requires --workflow', output.getvalue())
        self.assertEqual(self.fixture.commands, [])

    def test_browser_environment_forwards_only_playwright_paths(self):
        with mock.patch.dict(SMOKE.os.environ, {
                'PLAYWRIGHT_CORE_PATH': '/runner/node_modules/playwright-core',
                'PLAYWRIGHT_BROWSERS_PATH': '/runner/playwright-browsers',
                'DATABASE_URL': 'postgres://must-not-leak'}, clear=False):
            env = SMOKE.browser_environment({'PATH': '/usr/bin'}, 'http://127.0.0.1:1234',
                                            {'owner': 'fixture-token'}, 'fixture project',
                                            Path('/tmp/screenshots'))
        self.assertEqual(env['PLAYWRIGHT_CORE_PATH'], '/runner/node_modules/playwright-core')
        self.assertEqual(env['PLAYWRIGHT_BROWSERS_PATH'], '/runner/playwright-browsers')
        self.assertNotIn('DATABASE_URL', env)
        self.assertEqual(env['FUXI_BROWSER_BASE_URL'], 'http://127.0.0.1:1234')

    def test_failed_cli_replaces_a_previous_pass_report(self):
        self.fixture.local_missing = True
        with tempfile.TemporaryDirectory() as temporary:
            report = Path(temporary) / 'result.json'
            report.write_text('{"status":"passed"}')
            args = ['smoke-production-image.py', '--image', 'missing-release', '--output', str(report)]
            with mock.patch.object(SMOKE.sys, 'argv', args), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit):
                    SMOKE.main()
            result = json.loads(report.read_text())
            self.assertEqual(result['status'], 'failed')
            self.assertEqual(result['image'], 'missing-release')
            self.assertIn('not available locally', result['error'])


class ManifestValidationTests(unittest.TestCase):
    def test_missing_duplicate_or_malformed_entries_rejected(self):
        fixtures = [None, [], {}, {'assetVersion': 'z' * 64, 'assets': MANIFEST['assets']},
                    {'assetVersion': VERSION, 'assets': []},
                    {'assetVersion': VERSION, 'assets': [None]},
                    {'assetVersion': VERSION, 'assets': [{'path': []}]}]
        duplicate = copy.deepcopy(MANIFEST)
        duplicate['assets'].append(copy.deepcopy(duplicate['assets'][0]))
        fixtures.append(duplicate)
        for key, value in (('sha256', 'bad'), ('size', 0), ('size', True), ('size', '12')):
            manifest = copy.deepcopy(MANIFEST)
            manifest['assets'][0][key] = value
            fixtures.append(manifest)
        for manifest in fixtures:
            with self.subTest(manifest=manifest):
                with self.assertRaises(RuntimeError):
                    SMOKE.verified_asset_entries(manifest)

    def test_source_gate_rejects_forged_or_incomplete_clean_metadata(self):
        for source in (None, {'commit': 'not-a-commit', 'workingTreeClean': True},
                       {'commit': 'a1' * 20, 'workingTreeClean': 'true'},
                       {'commit': 'a1' * 20, 'workingTreeClean': False}):
            with self.subTest(source=source):
                manifest = {'source': source, 'productionSourceEligible': True}
                with self.assertRaisesRegex(RuntimeError, 'clean, tracked'):
                    SMOKE.source_eligibility(manifest, required=True)
                self.assertFalse(SMOKE.source_eligibility(manifest)[1])

    def test_valid_extra_asset_does_not_replace_required_assets(self):
        manifest = copy.deepcopy(MANIFEST)
        manifest['assets'].append({'path': 'style/workbench.css', 'sha256': 'irrelevant'})
        version, entries = SMOKE.verified_asset_entries(manifest)
        self.assertEqual(version, VERSION)
        self.assertEqual(set(entries), set(ASSETS))


if __name__ == '__main__':
    unittest.main()
