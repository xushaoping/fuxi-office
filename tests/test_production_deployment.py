"""Deployment contracts: Compose rendering and offline helper behavior, no daemon/DB use."""
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
TEMPLATES = ('docker-compose.yml', 'docker-compose.prebuilt.yml', 'docker-compose.multi.yml')
OPTIONAL = [
    'OPENAI_MAX_COMPLETION_TOKENS', 'OPENAI_MAX_ESTIMATED_CALL_COST_USD',
    'OPENAI_MAX_PROJECT_DAILY_TOKENS', 'OPENAI_MAX_PROJECT_DAILY_ESTIMATED_COST_USD',
    'OPENAI_MAX_ORCHESTRATION_ESTIMATED_COST_USD', 'OPENAI_MAX_ORCHESTRATION_OBSERVED_TOKENS',
    'OPENAI_MODEL_MAX_CONCURRENCY', 'OPENAI_MODEL_SELECTION_POLICY',
    'OPENAI_MODEL_CANDIDATES_JSON', 'OPENAI_REQUEST_OPTIONS_JSON', 'OPENAI_FALLBACK_MODELS',
    'OPENAI_OPTIMIZATION_MODEL', 'DRIFT_WEBHOOK_ALLOWED_HOSTS', 'DRIFT_WEBHOOK_SIGNING_SECRET',
    'OPTIMIZATION_AUTODRAFT_PROJECT_IDS',
]
for task in ('ANALYSIS', 'PLAN', 'DSL', 'CHANGE_INTENT'):
    OPTIONAL.extend(f'OPENAI_{task}_{suffix}' for suffix in (
        'MODEL', 'MODEL_SELECTION_POLICY', 'REQUEST_OPTIONS_JSON', 'FALLBACK_MODELS',
        'EXPERIMENT_NAME', 'EXPERIMENT_MODEL', 'EXPERIMENT_TREATMENT_PERCENT'))
OPTIONAL.extend(f'OPENAI_ANALYSIS_{suffix}' for suffix in (
    'SIMPLE_MODEL', 'COMPLEX_MODEL', 'EXPERIMENT_SIMPLE_MODEL', 'EXPERIMENT_COMPLEX_MODEL'))
DEFAULTS = dict(DB_PASSWORD='fixture_runtime_682b', DB_ADMIN_PASSWORD='fixture_admin_93da',
                JWT_SECRET='491f787b3fcb407ba482ec643185a438', ADMIN_PASSWORD='fixture_login_72ba',
                APP_PORT='31847')


def isolated_env():
    # Never inherit real credentials or configuration from a developer shell.
    return {k: v for k, v in os.environ.items() if not k.startswith((
        'OPENAI_', 'FUXI_', 'EMBEDDING_', 'BINARYEN_', 'DRIFT_', 'AGENT_', 'DECISION_',
        'OPTIMIZATION_', 'SHUTDOWN_', 'REQUIRE_HISTORICAL_')) and k not in {
        'DB_PASSWORD', 'DB_ADMIN_PASSWORD', 'JWT_SECRET', 'ADMIN_PASSWORD', 'APP_ENV',
        'APP_PORT', 'APP_DB_USER', 'APP_DB_OWNER', 'TRUST_PROXY_HEADERS', 'CAPTCHA_POW_DIFFICULTY'}}


class ComposeContracts(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which('docker'):
            raise unittest.SkipTest('docker compose CLI unavailable; no daemon is required')
        probe = subprocess.run(['docker', 'compose', 'version'], capture_output=True, timeout=10)
        if probe.returncode:
            raise unittest.SkipTest('docker compose CLI unavailable')

    def render(self, template, overrides=None, remove=()):
        values = dict(DEFAULTS)
        values.update(overrides or {})
        for key in remove:
            values.pop(key, None)
        with tempfile.TemporaryDirectory() as temp:
            env_file = Path(temp) / 'fixture.env'
            env_file.write_text(''.join(f'{key}={value}\n' for key, value in values.items()))
            return subprocess.run(['docker', 'compose', '-p', 'fixture-deployment', '--env-file',
                                   str(env_file), '-f', str(ROOT / template), 'config', '--format', 'json'],
                                  cwd=ROOT, env=isolated_env(), capture_output=True, text=True, timeout=20)

    def config(self, template, **kwargs):
        result = self.render(template, **kwargs)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def test_complete_configuration_survives_env_file(self):
        sentinels = {key: f'fixture_{i}' for i, key in enumerate(OPTIONAL)}
        sentinels.update({
            'AGENT_ORCHESTRATION_STALE_AFTER_SECONDS': '239',
            'OPENAI_API_KEY': 'fixture_api', 'OPENAI_BASE_URL': 'https://fixture.invalid/v1',
            'OPENAI_MODEL': 'fixture-model', 'EMBEDDING_ENABLED': 'true',
            'FUXI_EMBEDDING_ENABLED': 'true', 'FUXI_EMBEDDING_PROVIDER': 'local',
            'OPENAI_EMBEDDING_MODEL': 'fixture-vector', 'OPENAI_API_BASE': 'https://vectors.invalid/v1',
            'DECISION_COPILOT_ENABLED': 'false', 'OPTIMIZATION_AUTODRAFT_MODE': 'shadow',
            'REQUIRE_HISTORICAL_REGRESSION_BEFORE_PUBLISH': 'false',
        })
        for template in TEMPLATES:
            with self.subTest(template=template):
                app_env = self.config(template, overrides=sentinels)['services']['app']['environment']
                self.assertEqual({key: app_env.get(key) for key in sentinels}, sentinels)

    def test_absent_optional_variables_are_unset_and_explicit_empty_is_preserved(self):
        for template in TEMPLATES:
            with self.subTest(template=template):
                env = self.config(template)['services']['app']['environment']
                for key in OPTIONAL:
                    self.assertIsNone(env.get(key), key)
                budget = 'OPENAI_MAX_PROJECT_DAILY_TOKENS'
                env = self.config(template, overrides={budget: ''})['services']['app']['environment']
                self.assertEqual(env[budget], '')  # Rust must reject an explicitly empty budget.

    def test_production_runtime_and_database_boundary(self):
        for template in TEMPLATES:
            with self.subTest(template=template):
                config = self.config(template)
                app, pg = (config['services'][key] for key in ('app', 'postgres'))
                self.assertEqual(app['environment']['APP_ENV'], 'production')
                self.assertEqual(app['environment']['SHUTDOWN_GRACE_SECONDS'], '30')
                self.assertEqual(app['environment']['SHUTDOWN_DRAIN_SECONDS'], '2')
                self.assertEqual(app['stop_grace_period'], '45s')
                self.assertIn('http://localhost:3000/api/ready', app['healthcheck']['test'])
                self.assertTrue(all(port['host_ip'] == '127.0.0.1' for port in app['ports']))
                self.assertEqual(pg['image'], 'pgvector/pgvector:pg18')
                self.assertEqual(pg['environment']['POSTGRES_USER'], 'postgres')
                self.assertEqual(pg['environment']['POSTGRES_PASSWORD'], DEFAULTS['DB_ADMIN_PASSWORD'])
                self.assertNotIn('DB_ADMIN_PASSWORD', app['environment'])
                self.assertNotIn(DEFAULTS['DB_ADMIN_PASSWORD'], app['environment']['DATABASE_URL'])
                self.assertIn('://fuxi:', app['environment']['DATABASE_URL'])
                mounts = {mount['target']: mount for mount in pg['volumes']}
                self.assertIn('/var/lib/postgresql', mounts)
                self.assertNotIn('/var/lib/postgresql/data', mounts)
                self.assertTrue(mounts['/opt/fuxi-db-scripts']['read_only'])
                self.assertEqual(Path(mounts['/docker-entrypoint-initdb.d/001-bootstrap.sh']['source']).name,
                                 'bootstrap-production-db.sh')

    def test_admin_password_is_mandatory_and_runtime_role_can_be_renamed(self):
        for template in TEMPLATES:
            with self.subTest(template=template):
                result = self.render(template, remove=('DB_ADMIN_PASSWORD',))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('DB_ADMIN_PASSWORD', result.stderr)
                config = self.config(template, overrides={'APP_DB_USER': 'runtime_fixture'})
                self.assertIn('://runtime_fixture:', config['services']['app']['environment']['DATABASE_URL'])
                self.assertEqual(config['services']['postgres']['environment']['APP_DB_USER'], 'runtime_fixture')

    def test_clean_build_does_not_require_ignored_archive(self):
        for template in ('docker-compose.yml', 'docker-compose.multi.yml'):
            args = self.config(template)['services']['app']['build']['args']
            self.assertEqual(args['BINARYEN_ARCHIVE'], '')
            self.assertEqual(args['BINARYEN_VERSION'], '132')
            self.assertEqual(args['BINARYEN_MAX_TIME'], '1800')

    def test_bootstrap_and_login_ui_do_not_expose_shared_demo_credentials(self):
        init_sql = (ROOT / 'script/init.sql').read_text()
        login_page = (ROOT / 'src/components/auth/login_page.rs').read_text()
        readme = (ROOT / 'README.md').read_text()
        pdf_generator = (ROOT / 'script/generate_project_pdf.py').read_text()
        self.assertNotIn('admin1234', init_sql)
        self.assertNotIn('INSERT INTO users', init_sql)
        self.assertNotIn('admin/admin123', login_page)
        self.assertNotIn('fuxi/admin123', login_page)
        self.assertNotIn('admin/admin123', readme)
        self.assertNotIn('fuxi/admin123', readme)
        self.assertNotIn('admin1234', pdf_generator)
        self.assertIn('ensure_admin_user(&pool)', (ROOT / 'src/main.rs').read_text())

    def test_docker_dependencies_use_bounded_verified_apt_phases(self):
        dockerfile = (ROOT / 'Dockerfile').read_text()
        installer = (ROOT / 'script/install-debian-packages.sh').read_text()
        self.assertGreaterEqual(dockerfile.count('bash /build-tools/install-debian-packages.sh'), 2)
        for key, default in (('APT_UPDATE_TIMEOUT', '180'), ('APT_INSTALL_TIMEOUT', '600'),
                             ('APT_NETWORK_TIMEOUT', '20'), ('APT_DOWNLOAD_RETRIES', '2')):
            self.assertIn(f'ARG {key}={default}\n', dockerfile)
            self.assertGreaterEqual(dockerfile.count(f'ARG {key}\n'), 2)
        self.assertIn("APT::Update::Error-Mode=any", installer)
        self.assertIn('Acquire::http::Timeout=', installer)
        self.assertIn('Acquire::https::Timeout=', installer)
        self.assertIn('timeout --signal=TERM --kill-after=10s', installer)
        self.assertIn('Debian dependency phase failed: %s (exit %s)', installer)


class HelperContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'script').mkdir()
        (self.root / 'bin').mkdir()
        self.env = isolated_env()
        self.env.update(PATH=str(self.root / 'bin') + os.pathsep + os.environ['PATH'],
                        FIXTURE_ROOT=str(self.root), TMPDIR=str(self.root))

    def executable(self, name, contents):
        path = self.root / 'bin' / name
        path.write_text('#!/usr/bin/env bash\nset -euo pipefail\n' + contents)
        path.chmod(0o700)

    def multi(self, command, instance='app-a', failure=False):
        shutil.copy(ROOT / 'script/multi-instance.sh', self.root / 'script/')
        (self.root / 'docker-compose.multi.yml').write_text('services: {}\n')
        env_dir = self.root / 'deploy/multi-instance'
        env_dir.mkdir(parents=True, exist_ok=True)
        env_dir.joinpath('app-a.env').write_text(''.join(f'{k}={v}\n' for k, v in DEFAULTS.items()))
        self.executable('docker', 'printf "%s\\n" "$*" >> "$FIXTURE_ROOT/docker.log"\n'
                        'printf "fixture-dump\\n"\n' + ('exit 7\n' if failure else ''))
        return subprocess.run(['bash', str(self.root / 'script/multi-instance.sh'), command, instance],
                              env=self.env, capture_output=True, text=True, timeout=10)

    def test_backup_is_private_and_atomically_published(self):
        result = self.multi('backup')
        self.assertEqual(result.returncode, 0, result.stderr)
        backups = list((self.root / 'backups').iterdir())
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].suffix, '.sql')
        self.assertEqual(backups[0].read_text(), 'fixture-dump\n')
        self.assertEqual(stat.S_IMODE(backups[0].stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(backups[0].parent.stat().st_mode), 0o700)
        self.assertIn('pg_dump -U postgres -d fuxi_dmn', (self.root / 'docker.log').read_text())

    def test_failed_dump_leaves_no_success_file_or_partial(self):
        result = self.multi('backup', failure=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(list((self.root / 'backups').iterdir()), [])
        self.assertNotIn('wrote', result.stdout)

    def test_instance_traversal_rejected_before_docker(self):
        result = self.multi('backup', instance='../app-a')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('invalid instance name', result.stderr)
        self.assertFalse((self.root / 'docker.log').exists())

    def test_instance_http_redirects_without_plaintext_proxy(self):
        result = self.multi('nginx-snippet')
        self.assertEqual(result.returncode, 0, result.stderr)
        http = result.stdout.split('listen 80;', 1)[1].split('# HTTPS server', 1)[0]
        self.assertIn('return 308 https://$host$request_uri;', http)
        self.assertNotIn('proxy_pass', http)
        self.assertIn('server 127.0.0.1:31847;', result.stdout)
        self.assertIn('X-Forwarded-For $remote_addr;', result.stdout)

    def test_upload_packages_repository_configs_and_never_executes_remote_commands(self):
        shutil.copy(ROOT / 'build-and-upload-image.sh', self.root)
        for name in ('docker-compose.prebuilt.yml', 'nginx.conf', '.env.production.example'):
            shutil.copy(ROOT / name, self.root)
        (self.root / '.env').write_text('PRIVATE_SENTINEL=must-not-upload\n')
        (self.root / 'script/bootstrap-production-db.sh').write_text('# fixture bootstrap\n')
        (self.root / 'script/init.sql').write_text('-- fixture SQL\n')
        (self.root / 'script/ignored.tar.gz').write_text('not-in-release')
        self.executable('docker', 'printf "%s\\n" "$*" >> "$FIXTURE_ROOT/docker.log"\n'
                        'case "$1" in save) printf "image-fixture\\n";; image) printf "sha256:fixture\\n";; esac\n')
        self.executable('git', 'case "$3" in rev-parse) printf "fixture-commit\\n";; status) :;; esac\n')
        self.executable('scp', 'cp "$1" "$FIXTURE_ROOT/captured.tar.gz"\n')
        self.executable('ssh', 'echo unexpected-remote-execution >&2; exit 91\n')
        result = subprocess.run(['bash', str(self.root / 'build-and-upload-image.sh'), 'fixture@host'],
                                env=self.env, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        with tarfile.open(self.root / 'captured.tar.gz') as archive:
            names = archive.getnames()
            self.assertIn('fuxi-release/script/init.sql', names)
            self.assertIn('fuxi-release/SHA256SUMS', names)
            self.assertNotIn('fuxi-release/.env', names)
            self.assertNotIn('fuxi-release/script/ignored.tar.gz', names)
            for name in ('docker-compose.prebuilt.yml', 'nginx.conf'):
                self.assertEqual(archive.extractfile('fuxi-release/' + name).read(), (ROOT / name).read_bytes())
        log = (self.root / 'docker.log').read_text()
        self.assertNotIn('compose', log)
        self.assertNotIn('load', log)
        self.assertIn('FUXI_BUILD_COMMIT=fixture-commit', log)
        self.assertEqual(list(self.root.glob('fuxi-release.*')), [])

    def test_upload_failure_cleans_private_staging(self):
        shutil.copy(ROOT / 'build-and-upload-image.sh', self.root)
        for name in ('docker-compose.prebuilt.yml', 'nginx.conf', '.env.production.example'):
            shutil.copy(ROOT / name, self.root)
        (self.root / 'script/bootstrap-production-db.sh').write_text('# fixture\n')
        self.executable('git', 'printf "fixture\\n"\n')
        self.executable('docker', 'case "$1" in save) printf "partial\\n"; exit 7;; esac\n')
        self.executable('scp', 'touch "$FIXTURE_ROOT/unexpected-upload"\n')
        result = subprocess.run(['bash', str(self.root / 'build-and-upload-image.sh'), 'fixture@host'],
                                env=self.env, capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / 'unexpected-upload').exists())
        self.assertEqual(list(self.root.glob('fuxi-release.*')), [])

    def test_repository_http_servers_only_redirect(self):
        config = (ROOT / 'nginx.conf').read_text()
        servers = re.findall(r'server \{\s*listen 80;(.*?)\n    \}', config, re.S)
        self.assertEqual(len(servers), 2)
        for server in servers:
            self.assertIn('return 308 https://$host$request_uri;', server)
            self.assertNotIn('proxy_pass', server)
        self.assertNotIn('$proxy_add_x_forwarded_for', config)

    def test_binaryen_pins_both_official_architectures_and_rejects_unverified_custom_input(self):
        dockerfile = (ROOT / 'Dockerfile').read_text()
        start = dockerfile.index('    set -eu;', dockerfile.index('ARG BINARYEN_VERSION='))
        end = dockerfile.index('    BINARYEN_VERSION="${BINARYEN_VERSION}"', start)
        # Execute the Dockerfile checksum-selection shell without downloading/building anything.
        selection = dockerfile[start:end].replace('\\\n', '\n')
        hashes = {'x86_64': '195ddc94f9bc89f45abdabb0b9eea86023d727ba90eac8b35b80f2544fc30572',
                  'aarch64': 'c58562417836c5d0493d89bdefc434933bdc097db641b483df86bcfa557a107f'}
        for arch, digest in hashes.items():
            self.executable('uname', f'echo {arch}\n')
            env = dict(self.env, BINARYEN_VERSION='132', BINARYEN_ARCHIVE='', BINARYEN_DOWNLOAD_URL='', BINARYEN_SHA256='')
            result = subprocess.run(['bash', '-c', selection + '\nprintf "%s" "$BINARYEN_SHA256"'],
                                    env=env, capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, digest)
            for key, value in (('BINARYEN_VERSION', '133'), ('BINARYEN_ARCHIVE', 'local.tar.gz'),
                               ('BINARYEN_DOWNLOAD_URL', 'https://fixture.invalid/custom')):
                failed = subprocess.run(['bash', '-c', selection], env=dict(env, **{key: value}),
                                        capture_output=True, text=True, timeout=5)
                self.assertNotEqual(failed.returncode, 0)
                self.assertIn('explicit BINARYEN_SHA256', failed.stderr)
        self.assertIn('ENV APP_ENV=production', dockerfile)
        self.assertIn('ARG BINARYEN_MAX_TIME=1800', dockerfile)
        self.assertIn('    python3 \\', dockerfile)


if __name__ == '__main__':
    unittest.main()
