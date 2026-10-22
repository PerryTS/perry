#!/usr/bin/env python3
"""Exercise actual npm staging with complete and incomplete component payloads."""
import io
import os
import tarfile
import zipfile
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import component_release_check as release

ROOT = Path(__file__).resolve().parent.parent
TARGETS = [
    ('darwin-arm64', 'perry-macos-aarch64'),
    ('linux-x64', 'perry-linux-x86_64'),
    ('win32-x64', 'perry-windows-x86_64'),
]


class ComponentPackages(unittest.TestCase):
    def fixture(self, suffix, artifact, missing=None):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        root = Path(work.name)
        repo, payload = root/'repo', root/'artifacts'/artifact
        payload.mkdir(parents=True)
        paths = ['scripts/stage-npm.sh', 'npm/perry/package.json.tmpl',
                 'npm/perry/bin/perry.js', 'npm/perry/bin/perry-wit.js',
                 f'npm/perry-{suffix}/package.json.tmpl']
        for name in paths:
            dest = repo/name
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT/name, dest)
        (repo/'Cargo.toml').write_text('[workspace.package]\nversion = "0.0.0-test"\n')
        windows = suffix.startswith('win32-')
        tool = 'perry-wit.exe' if windows else 'perry-wit'
        engine = 'perry_component_host.lib' if windows else 'libperry_component_host.a'
        names = ['perry.exe', 'LLVM-C.dll'] if windows else ['perry']
        if suffix in {t[0] for t in TARGETS}:
            names += [tool, engine]
        for name in names:
            if name != missing:
                (payload/name).write_text('artifact ' + name + '\n')
        result = subprocess.run(['bash', str(repo/'scripts/stage-npm.sh'), str(root/'artifacts')],
            env=dict(os.environ, SKIP_MISSING='1', PERRY_NPM_NO_COMPRESS='1'),
            capture_output=True, text=True, timeout=30)
        return result, repo/f'npm/perry-{suffix}', payload, tool, engine

    def test_component_tool_and_engine_survive_staging_on_each_shipped_target(self):
        for suffix, artifact in TARGETS:
            with self.subTest(suffix=suffix):
                result, package, payload, tool, engine = self.fixture(suffix, artifact)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                for location, name in [('bin',tool), ('lib',engine)]:
                    self.assertEqual((package/location/name).read_bytes(), (payload/name).read_bytes())

    def test_missing_member_of_component_pair_stops_staging(self):
        for suffix, artifact in TARGETS:
            names = ('perry-wit.exe', 'perry_component_host.lib') if suffix.startswith('win32') else ('perry-wit', 'libperry_component_host.a')
            for missing in names:
                with self.subTest(suffix=suffix, missing=missing):
                    result, *_ = self.fixture(suffix, artifact, missing)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('missing required component artifact ' + missing, result.stderr)

    def test_other_platform_packages_keep_their_existing_payload_contract(self):
        result, *_ = self.fixture('linux-arm64', 'perry-linux-aarch64')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


class ArchivePayload(unittest.TestCase):
    def test_partial_duplicate_and_failed_execution_cannot_qualify_archive(self):
        for expected, count in [(release.NATIVE_PHASES, 20), (release.COMMAND_PHASES, 5)]:
            self.assertEqual(len(expected), count)
            rows = [{'name': name, 'exitCode': 0} for name in sorted(expected)]
            release.require_phases(rows, expected)
            for bad in (rows[:-1], rows[:-1] + rows[:1], rows + rows[:1],
                        [dict(row, exitCode=1) for row in rows]):
                with self.assertRaises(RuntimeError):
                    release.require_phases(bad, expected)

    def test_tar_and_zip_extract_the_complete_host_payload_only(self):
        for target in release.TARGETS:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                names = release.required(target)
                if target.endswith('-msvc'):
                    archive = root/'release.zip'
                    with zipfile.ZipFile(archive, 'w') as writer:
                        for name in names + ['../unrelated']:
                            writer.writestr(name, ('payload ' + name).encode())
                else:
                    archive = root/'release.tar.gz'
                    with tarfile.open(archive, 'w:gz') as writer:
                        for name in names + ['../unrelated']:
                            data = ('payload ' + name).encode()
                            member = tarfile.TarInfo('./' + name)
                            member.size = len(data)
                            writer.addfile(member, io.BytesIO(data))
                files = release.extract(archive, root/'install', target)
                self.assertEqual(set(files), set(names))
                self.assertFalse((root/'unrelated').exists())
                for name in names:
                    self.assertEqual((root/'install'/name).read_bytes(), ('payload ' + name).encode())

    def test_missing_duplicate_and_symlink_required_members_fail(self):
        target = 'x86_64-unknown-linux-gnu'
        for broken in ('missing', 'duplicate', 'symlink', 'empty'):
            with self.subTest(broken=broken), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root/'release.tar.gz'
                names = release.required(target)
                if broken == 'missing':
                    names.remove('libperry_component_host.a')
                if broken == 'duplicate':
                    names.append('perry-wit')
                with tarfile.open(archive, 'w:gz') as writer:
                    for name in names:
                        member = tarfile.TarInfo(name)
                        data = b'payload'
                        if name == 'perry-wit' and broken == 'symlink':
                            member.type, member.linkname = tarfile.SYMTYPE, '/elsewhere/perry-wit'
                            data = b''
                        if name == 'perry-wit' and broken == 'empty':
                            data = b''
                        member.size = len(data)
                        writer.addfile(member, io.BytesIO(data))
                with self.assertRaises(ValueError):
                    release.extract(archive, root/'install', target)


if __name__ == '__main__':
    unittest.main()
