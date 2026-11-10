"""Exercise the actual release workflow shell with read-only API/CLI doubles."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from tests.paths.release_pipeline import (
    LLVM_SETUP_ACTION,
    PUBLISH_VISIBILITY_SCRIPT,
    RELEASE_WORKFLOW,
    ROOT,
    TEST_WORKFLOW,
)

JOBS = yaml.safe_load(RELEASE_WORKFLOW.read_text())['jobs']
SHA, BASE = 'a' * 40, 'b' * 40


def step(job, name):
    return next(s['run'] for s in JOBS[job]['steps'] if s.get('name', '').startswith(name))


def shell(script, *args, cwd=ROOT, env=None):
    return subprocess.run(['bash', '-euo', 'pipefail', '-c', script, 'test', *args],
                          cwd=cwd, env={**os.environ, **(env or {})},
                          capture_output=True, text=True, timeout=10)


class ReleasePipeline(unittest.TestCase):
    def test_manifest_version_is_separate_from_download_tag(self):
        source = step('preflight', 'Resolve run mode')
        cases = [
            ('release', '', 'v1.2.3', 'release', 'v1.2.3', '1.2.3'),
            ('workflow_dispatch', '', '', 'stage', '', ''),
            ('workflow_dispatch', 'true', '', 'nightly',
             'nightly-v1.2.3-20261023-r433a1-gaaaaaaaa', '1.2.3-20261023-r433a1-gaaaaaaaa'),
            ('schedule', 'true', '', 'nightly',
             'nightly-v1.2.3-20261023-r433a1-gaaaaaaaa', '1.2.3-20261023-r433a1-gaaaaaaaa'),
        ]
        for sha in [SHA, '01234567' + 'a' * 32]:
            for event, nightly, release_tag, mode, tag, version in cases:
                with self.subTest(event=event, nightly=nightly, sha=sha), tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp)
                    (root / 'Cargo.toml').write_text('[workspace.package]\nversion = "1.2.3"\n')
                    output = root / 'output'
                    result = shell('date() { echo 20261023; }\n' + source, cwd=root, env={
                        'EVENT': event, 'NIGHTLY': nightly, 'RELEASE_TAG': release_tag,
                        'CUT_RELEASE': '', 'EXISTING_TAG': '', 'CANDIDATE_SHA': sha,
                        'SHA': sha, 'REF': 'refs/heads/main', 'REF_NAME': 'main',
                        'RUN_NUMBER': '433', 'RUN_ATTEMPT': '1', 'GITHUB_OUTPUT': str(output),
                    })
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    values = dict(line.split('=', 1) for line in output.read_text().splitlines())
                    self.assertEqual((values['mode'], values['tag'], values['version']),
                                     (mode, tag.replace('aaaaaaaa', sha[:8]), version.replace('aaaaaaaa', sha[:8])))
                    self.assertEqual(values['checkout-sha'], sha)

        # Both shells consume the resolved semver, while the artifact URL keeps
        # the tag. In particular, neither reconstructs a version from that URL.
        for name, argument in [('Unix', '"$RELEASE_VERSION"'),
                               ('Windows', '$env:RELEASE_VERSION')]:
            signing = next(s for s in JOBS['build']['steps']
                           if s.get('name') == f'Sign authenticated CLI update manifest ({name})')
            self.assertEqual(signing['env']['RELEASE_VERSION'], '${{ needs.preflight.outputs.version }}')
            self.assertIn('--release-version ' + argument, signing['run'])

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
    *'/jobs?'*) echo '{"jobs":[{"name":"full-suite-gate","conclusion":"success"}]}' ;;
    *'/git/ref/heads/'*) echo "$SHA" ;;
    'workflow run compiler-runtime.yml'*) echo dispatched ;;
    *'compiler-runtime.yml/runs?'*)
      n=$(cat "$CALLS.sim" 2>/dev/null || echo 0); echo $((n + 1)) > "$CALLS.sim"
      if [ "$n" = 0 ]; then echo '{"workflow_runs":[]}'
      else echo '{"workflow_runs":[{"id":77,"head_sha":"'"$SHA"'","status":"completed","conclusion":"success","html_url":"sim-success"}]}'; fi ;;
    *"head_sha=$BASE"*) echo '{"workflow_runs":[{"id":42,"head_sha":"'"$BASE"'","status":"completed","conclusion":"success","html_url":"test-success"}]}' ;;
    *"head_sha=$SHA"*) echo '{"workflow_runs":[]}' ;;
    *) echo "unexpected gh call: $*" >&2; return 1 ;;
  esac
}
sleep() { echo 'unexpected wait' >&2; exit 99; }
'''
            # release_gate_runs.py pages the run's jobs through the `gh` binary,
            # which a shell function cannot stand in for.
            bin_dir = Path(tmp) / 'bin'
            bin_dir.mkdir()
            fake_gh = bin_dir / 'gh'
            fake_gh.write_text('#!/bin/sh\necho "$*" >> "$CALLS"\n'
                               'echo \'{"jobs":[{"name":"simctl-tests / simctl","status":"completed","conclusion":"success"}]}\'\n')
            fake_gh.chmod(0o755)
            result = shell(stub + source, env={'CALLS': str(log), 'SHA': SHA, 'BASE': BASE,
                'MODE': 'cut-release', 'REF_NAME': 'release/test', 'REPO': 'test/repo',
                'PATH': str(bin_dir) + os.pathsep + os.environ['PATH']})
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            calls = log.read_text()
            self.assertIn('workflow run compiler-runtime.yml', calls)
            self.assertIn('suite=simctl-tests', calls)
            self.assertNotIn('workflow run test.yml', calls)
            sim_queries = [c for c in calls.splitlines() if 'compiler-runtime.yml/runs?' in c]
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
  if [ "$1" = "$PUBLISH_VISIBILITY_SCRIPT" ]; then return 1; fi
  command node "$@"
}
npm() { printf '%s\n' "$*" >> "$CALLS"; }
'''
            result = shell(stub + source, cwd=root, env={'CALLS': str(root / 'calls'),
                'NPM_TOKEN': '', 'NODE_AUTH_TOKEN': '', 'NPM_AUTH_TOKEN': '', 'DIST_TAG': 'test',
                'PUBLISH_VISIBILITY_SCRIPT': PUBLISH_VISIBILITY_SCRIPT})
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            calls = (root / 'calls').read_text()
            self.assertIn('publish ./platform.tgz', calls)
            self.assertNotIn('publish ./wrapper.tgz', calls)

    def test_apt_install_still_gates_after_update_failure(self):
        workflows = [JOBS, yaml.safe_load(TEST_WORKFLOW.read_text())['jobs']]
        sources = [s['run'] for jobs in workflows for job in jobs.values()
                   for s in job.get('steps', [])
                   if 'sudo apt-get update' in s.get('run', '') and '_bad=' in s['run']]
        action = yaml.safe_load(LLVM_SETUP_ACTION.read_text())
        sources += [s['run'] for s in action['runs']['steps']
                    if 'sudo apt-get update' in s.get('run', '')]
        self.assertEqual(len(sources), 10, 'exercise every changed apt site')
        for source in sources:
            start = source.index('if _bad=')
            lines = source[start:].splitlines()
            cached = 'install_online() {' in source
            if cached:
                end = lines.index(
                    'if [ "$mirror_ok" -ne 1 ]; then install_offline; fi'
                )
            else:
                end = next(i for i, line in enumerate(lines)
                           if 'apt-get install' in line and 'sudo' in line)
                while lines[end].rstrip().endswith('\\'):
                    end += 1
            fragment = '\n'.join(lines[:end + 1])
            # online rc, offline rc, populated cache, expected final rc
            cases = [(0, 0, False, 0), (42, 42, cached, 42)]
            if cached:
                cases += [(42, 0, True, 0), (42, 0, False, 1)]
            for install_rc, offline_rc, warm, expected in cases:
                with self.subTest(case=(cached, install_rc, offline_rc, warm)), tempfile.TemporaryDirectory() as tmp:
                    cache = Path(tmp) / 'debs'
                    cache.mkdir()
                    if cached and warm:
                        for pkg in ('llvm-22-dev', 'llvm-22-tools',
                                    'libpolly-22-dev', 'clang-22'):
                            (cache / f'{pkg}_fixture.deb').touch()
                    stub = r'''
sudo() {
  case "$*" in
    'grep '*) return 1 ;;
    'apt-get update'*) return 100 ;;
    *'apt-get install'*'.deb'*)
      echo OFFLINE_REACHED; return "$OFFLINE_RC" ;;
    *'apt-get install'*) echo INSTALL_REACHED; return "$INSTALL_RC" ;;
    'rm -rf '*) return 0 ;;
    *) echo "unexpected sudo: $*" >&2; return 99 ;;
  esac
}
apt-cache() { echo 'Candidate: 22.1.5'; }
getent() { return 1; }
sleep() { :; }
'''
                    result = shell(stub + fragment, cwd=Path(tmp), env={
                        'INSTALL_RC': str(install_rc),
                        'OFFLINE_RC': str(offline_rc),
                        'LLVM_DEBS': str(cache),
                        'LLVM_DEBS_EXACT_HIT': 'true',
                        'TMPDIR': tmp,
                    })
                    self.assertEqual(result.returncode, expected,
                                     result.stdout + result.stderr)
                    self.assertIn('INSTALL_REACHED', result.stdout)
                    self.assertEqual(
                        'OFFLINE_REACHED' in result.stdout,
                        cached and warm and install_rc != 0,
                    )
                    if cached and install_rc and not warm:
                        self.assertIn('cache is cold', result.stdout)

    def test_release_notes_cap_preserves_unicode_and_bounds(self):
        source = step('create-release', 'Create tag')
        fragment = source[source.index('LIMIT='):source.index('# Created with GITHUB_TOKEN:')]
        for body in ['short notes\n', 'Unicode: 日本語\n' * 100000, 'x' * 150000 + '\n']:
            with self.subTest(size=len(body)), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / 'changelog.d').mkdir()
                (root / 'release-notes-full.md').write_text(body)
                result = shell(fragment.replace('/tmp/', tmp + '/'), cwd=root,
                               env={'TAG': 'v0.0.1', 'REPO': 'test/repo',
                                    # The fragment starts at LIMIT=; the workflow sets
                                    # notes_dir just before it (release_notes_source.sh).
                                    'notes_dir': 'changelog.d'})
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                actual = (root / 'release-notes.md').read_text()
                self.assertLess(len(actual.encode()), 125000)
                if len(body.encode()) > 120000:
                    self.assertIn('These notes are truncated.', actual)
                else:
                    self.assertEqual(actual, body)


if __name__ == '__main__':
    unittest.main()
