"""Exercise the actual release workflow shell with read-only API/CLI doubles."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]
JOBS = yaml.safe_load((ROOT / '.github/workflows/release-packages.yml').read_text())['jobs']
SHA, BASE = 'a' * 40, 'b' * 40


def step(job, name):
    return next(s['run'] for s in JOBS[job]['steps'] if s.get('name', '').startswith(name))


def shell(script, *args, cwd=ROOT, env=None):
    return subprocess.run(['bash', '-euo', 'pipefail', '-c', script, 'test', *args],
                          cwd=cwd, env={**os.environ, **(env or {})},
                          capture_output=True, text=True, timeout=10)


class ReleasePipeline(unittest.TestCase):
    def test_ancestor_comparison_is_complete_and_safe(self):
        source = step('await-tests', 'Wait for Tests')
        function = source[source.index('  plumbing_only() {'):source.index('  if gate_green "$SHA";')]
        good = dict(status='ahead', behind_by=0, base_commit={'sha': BASE},
                    merge_base_commit={'sha': BASE},
                    files=[dict(filename='changelog.d/fix.md', status='added')])
        cases = [('good', good, True)]
        for path in ['crates/runtime.rs', '.github/workflows/test.yml', 'Cargo.toml',
                     'changelog.d/../crates/unsafe.rs', 'changelog.d//x',
                     'changelog.d/x\ny', 'scripts/linux-x/y.Dockerfile']:
            cases.append((path, {**good, 'files': [dict(filename=path, status='modified')]}, False))
        for path in ['scripts/linux-glibc-2.31.Dockerfile', '.github/workflows/release-packages.yml']:
            cases.append((path, {**good, 'files': [dict(filename=path, status='modified')]}, True))
        for count in [0, 299, 300, 301]:
            cases.append((str(count), {**good, 'files': good['files'] * count}, 0 < count < 300))
        for old, allowed in [('crates/runtime.rs', False), ('changelog.d/old.md', True), (None, False)]:
            cases.append(('rename ' + str(old), {**good, 'files': [dict(
                filename='changelog.d/new.md', status='renamed', previous_filename=old)]}, allowed))
        for patch in [dict(status='diverged'), dict(behind_by=1), dict(files=None),
                      dict(base_commit={'sha': SHA}), dict(merge_base_commit={'sha': SHA}),
                      dict(files=[dict(filename='changelog.d/x', status='unknown')])]:
            cases.append((str(patch), {**good, **patch}, False))
        for name, data, expected in cases:
            with self.subTest(name=name):
                result = shell(function + '\nplumbing_only "$1" "$2"', json.dumps(data), BASE)
                self.assertEqual(result.returncode == 0, expected, result.stderr)
        self.assertNotEqual(shell(function + '\nplumbing_only "$1" "$2"', '{bad', BASE).returncode, 0)

    def test_tag_lookup_fails_closed(self):
        source = step('create-release', 'Create tag')
        fragment = source[source.index('existing=""'):source.index('./scripts/cut_release_notes.sh')]
        stub = 'gh() { printf "%s\\n" "$RESPONSE"; return "$RC"; }\n'
        cases = [(0, 'HTTP/2.0 200 OK\n\n' + SHA, 0, '1'),
                 (0, 'HTTP/2.0 200 OK\n\n' + BASE, 1, None),
                 (0, 'HTTP/2.0 200 OK\n\n12345678garbage', 1, None),
                 (0, 'HTTP/2.0 200 OK\n\n', 1, None),
                 (1, 'HTTP/2.0 404 Not Found\n\n{"message":"Not Found"}', 0, '0'),
                 (1, 'HTTP/2.0 403 Forbidden', 1, None),
                 (1, 'HTTP/2.0 500 Server Error', 1, None),
                 (1, 'network failure', 1, None)]
        for rc, response, expected, reuse in cases:
            with self.subTest(response=response):
                result = shell(stub + fragment + '\necho "REUSE=$reuse_tag"', env={
                    'RC': str(rc), 'RESPONSE': response, 'SHA': SHA, 'TAG': 'v0.0.1', 'REPO': 'test/repo'})
                self.assertEqual(result.returncode, expected, result.stderr)
                if reuse is not None:
                    self.assertIn('REUSE=' + reuse, result.stdout)

    def test_ancestor_test_gate_keeps_simulator_on_candidate(self):
        source = step('await-tests', 'Wait for Tests')
        with tempfile.TemporaryDirectory() as tmp:
            log = Path(tmp) / 'calls'
            stub = r'''
gh() {
  printf '%s\n' "$*" >> "$CALLS"
  case "$*" in
    *'/commits?'*) printf '%s\n%s\n' "$SHA" "$BASE" ;;
    *'/compare/'*) printf '{"status":"ahead","behind_by":0,"base_commit":{"sha":"%s"},"merge_base_commit":{"sha":"%s"},"files":[{"filename":"changelog.d/fix.md","status":"added"}]}' "$BASE" "$BASE" ;;
    *'/jobs?'*) echo full-suite-gate ;;
    *'/git/ref/heads/'*) echo "$SHA" ;;
    'workflow run simctl-tests.yml'*) echo dispatched ;;
    *'simctl-tests.yml/runs?'*'per_page=1'*) echo 0 ;;
    *'simctl-tests.yml/runs?'*) echo '{"workflow_runs":[{"status":"completed","conclusion":"success","html_url":"sim-success"}]}' ;;
    *"head_sha=$BASE"*) echo '{"workflow_runs":[{"id":42,"status":"completed","conclusion":"success","html_url":"test-success"}]}' ;;
    *"head_sha=$SHA"*) echo '{"workflow_runs":[]}' ;;
    *) echo "unexpected gh call: $*" >&2; return 1 ;;
  esac
}
sleep() { echo 'unexpected wait' >&2; exit 99; }
'''
            result = shell(stub + source, env={'CALLS': str(log), 'SHA': SHA, 'BASE': BASE,
                'MODE': 'cut-release', 'REF_NAME': 'release/test', 'REPO': 'test/repo'})
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            calls = log.read_text()
            self.assertIn('workflow run simctl-tests.yml', calls)
            self.assertNotIn('workflow run test.yml', calls)
            sim_queries = [c for c in calls.splitlines() if 'simctl-tests.yml/runs?' in c]
            self.assertEqual(len(sim_queries), 2)
            self.assertTrue(all('head_sha=' + SHA in c for c in sim_queries), calls)

    def test_tarball_set_matches_names_not_just_count(self):
        source = step('npm-publish', 'Bundle exact npm')
        for names, expected in [(['perryts-perry-a-1.tgz', 'perryts-perry-1.tgz'], 0),
                                ([], 1),
                                (['perryts-perry-a-1.tgz'], 1),
                                (['perryts-perry-a-1.tgz', 'wrong-1.tgz'], 1),
                                (['perryts-perry-a-1.tgz', 'perryts-perry-1.tgz', 'extra.tgz'], 1)]:
            with self.subTest(names=names), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                package = root / 'npm' / 'fixture'
                package.mkdir(parents=True)
                for name in names:
                    (package / name).touch()
                (root / 'npm-publish-manifest.json').write_text(json.dumps({'packages': [
                    {'name': '@perryts/perry-a', 'version': '1'}, {'name': '@perryts/perry', 'version': '1'}]}))
                (root / 'socket-scan-receipt.json').write_text('{}')
                result = shell(source, cwd=root)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
                if expected:
                    self.assertIn('  on disk:', result.stderr)
                    self.assertNotIn('unbound variable', result.stderr)
                    for name in names:
                        self.assertIn('npm/fixture/' + name, result.stderr)

    def test_wrapper_is_withheld_when_visibility_fails(self):
        source = step('npm-publish', 'npm publish exact')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'npm-publish-manifest.json').write_text(json.dumps({'packages': [
                {'name': '@perryts/perry-a', 'version': '1', 'path': 'platform.tgz', 'sha1': SHA},
                {'name': '@perryts/perry', 'version': '1', 'path': 'wrapper.tgz', 'sha1': SHA}]}))
            stub = r'''
node() {
  if [ "$1" = scripts/publish/platform-visibility.mjs ]; then return 1; fi
  command node "$@"
}
npm() { printf '%s\n' "$*" >> "$CALLS"; }
'''
            result = shell(stub + source, cwd=root, env={'CALLS': str(root / 'calls'),
                'NPM_TOKEN': '', 'NODE_AUTH_TOKEN': '', 'NPM_AUTH_TOKEN': '', 'DIST_TAG': 'test'})
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            calls = (root / 'calls').read_text()
            self.assertIn('publish ./platform.tgz', calls)
            self.assertNotIn('publish ./wrapper.tgz', calls)

    def test_apt_install_still_gates_after_update_failure(self):
        workflows = [JOBS, yaml.safe_load((ROOT / '.github/workflows/test.yml').read_text())['jobs']]
        sources = [s['run'] for jobs in workflows for job in jobs.values()
                   for s in job.get('steps', [])
                   if 'sudo apt-get update' in s.get('run', '') and '_bad=' in s['run']]
        action = yaml.safe_load((ROOT / '.github/actions/setup-llvm22/action.yml').read_text())
        sources += [s['run'] for s in action['runs']['steps']
                    if 'sudo apt-get update' in s.get('run', '')]
        self.assertEqual(len(sources), 10, 'exercise every changed apt site')
        for source in sources:
            start = source.index('if _bad=')
            lines = source[start:].splitlines()
            end = next(i for i, line in enumerate(lines) if 'apt-get install' in line and 'sudo' in line)
            while lines[end].rstrip().endswith('\\'):
                end += 1
            fragment = '\n'.join(lines[:end + 1])
            for install_rc in [0, 42]:
                with self.subTest(fragment=fragment, install_rc=install_rc):
                    stub = r'''
sudo() {
  case "$*" in
    'grep '*) return 1 ;;
    'apt-get update'*) return 100 ;;
    *'apt-get install'*) echo INSTALL_REACHED; return "$INSTALL_RC" ;;
    *) echo "unexpected sudo: $*" >&2; return 99 ;;
  esac
}
apt-cache() { echo 'Candidate: 22.1.4'; }
sleep() { :; }
'''
                    result = shell(stub + fragment, env={'INSTALL_RC': str(install_rc)})
                    self.assertEqual(result.returncode, install_rc, result.stdout + result.stderr)
                    self.assertIn('INSTALL_REACHED', result.stdout)

    def test_release_notes_cap_preserves_unicode_and_bounds(self):
        source = step('create-release', 'Create tag')
        fragment = source[source.index('LIMIT='):source.index('# Created with GITHUB_TOKEN:')]
        for body in ['short notes\n', 'Unicode: 日本語\n' * 100000, 'x' * 150000 + '\n']:
            with self.subTest(size=len(body)), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / 'changelog.d').mkdir()
                (root / 'release-notes-full.md').write_text(body)
                result = shell(fragment.replace('/tmp/', tmp + '/'), cwd=root,
                               env={'TAG': 'v0.0.1', 'REPO': 'test/repo'})
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                actual = (root / 'release-notes.md').read_text()
                self.assertLess(len(actual.encode()), 125000)
                if len(body.encode()) > 120000:
                    self.assertIn('These notes are truncated.', actual)
                else:
                    self.assertEqual(actual, body)


if __name__ == '__main__':
    unittest.main()
