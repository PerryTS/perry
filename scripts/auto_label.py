#!/usr/bin/env python3
"""Label new issues and pull requests with the triage taxonomy.

Issues are labelled from their issue-form answers (.github/ISSUE_TEMPLATE/*.yml):
the target platforms give platform:* and the performance stage turns type:bug
into type:perf. Pull requests are labelled from their changed paths (area:*,
platform:*) and their Conventional Commit title prefix (type:*). effort:* and
impact:* stay a human triage decision.

Labels are only added, except the type:bug -> type:perf swap. Each item is labelled
at most once: items created in the last --hours are skipped when they already carry
an area: label (PRs) or a platform: label (issues), or when BOT has labelled them
before, so a maintainer's correction stands.

  python3 scripts/auto_label.py            # dry run: print what would change
  python3 scripts/auto_label.py --apply    # write labels (needs GH_TOKEN)
  python3 scripts/auto_label.py --self-test
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import subprocess
import sys
import unittest
from unittest import mock

COMPILER = ('perry-parser', 'perry-hir', 'perry-transform', 'perry-codegen', 'perry-wpfacts')
RUNTIME = ('perry-runtime', 'perry-abi', 'perry-realm-table', 'perry-ffi', 'perry-native-registration')
STDLIB = ('perry-stdlib', 'perry-ext-', 'perry-ui', 'perry-http-', 'perry-tls-session', 'perry-wasm-host',
          'perry-wasi', 'perry-component-', 'perry-wit', 'perry-updater', 'perry-update-manifest',
          'perry-audio-', 'perry-container-')
TOOLING = ('perry-doc-tests', 'perry-api-manifest')

# Crate-name prefix -> platform. The first match wins.
CRATE_PLATFORM = (
    ('perry-codegen-wasm', 'wasm'), ('perry-wasm-host', 'wasm'), ('perry-wasi', 'wasm'),
    ('perry-component-', 'wasm'), ('perry-wit', 'wasm'),
    ('perry-ui-ios', 'ios'), ('perry-codegen-swiftui', 'ios'),
    ('perry-ui-android', 'android'), ('perry-codegen-glance', 'android'), ('perry-codegen-wear-tiles', 'android'),
    ('perry-ui-windows', 'windows'), ('perry-ui-gtk4', 'linux'), ('perry-ui-macos', 'macos'),
)

TITLE_TYPE = {
    'fix': 'bug', 'feat': 'feature', 'perf': 'perf', 'size': 'size',
    'chore': 'chore', 'ci': 'chore', 'test': 'chore', 'tests': 'chore', 'docs': 'chore',
    'refactor': 'chore', 'style': 'chore', 'build': 'chore', 'changelog': 'chore', 'revert': 'chore',
    'deps': 'chore',
}
# A fix to the build or CI is maintenance, not a product bug.
CHORE_SCOPES = {'ci', 'deps', 'build'}

# Issue-form option text -> platform label value.
FORM_PLATFORM = {
    'macos': 'macos', 'linux': 'linux', 'windows': 'windows', 'ios': 'ios', 'ios-simulator': 'ios',
    'android': 'android', 'web': 'wasm', 'wasm': 'wasm',
}
HOST_PLATFORM = {'macOS': 'macos', 'Linux': 'linux', 'Windows': 'windows'}
PERF_STAGE = 'The program is much slower or uses much more memory than Node'
# The actor behind the workflow's github.token. Every workflow using that token labels as
# this actor, so a second labelling workflow must use another token or this script skips its items.
BOT = 'github-actions[bot]'


def crate_of(path: str) -> str | None:
    parts = path.split('/')
    return parts[1] if len(parts) > 2 and parts[0] == 'crates' else None


def area_of(path: str) -> str | None:
    if path.startswith(('.github/', 'scripts/')):
        return 'tooling-ci'
    if path == 'Cargo.lock':
        return 'deps'
    crate = crate_of(path)
    if crate is None:
        return None
    if crate == 'perry':
        return 'tooling-ci'
    for area, prefixes in (('compiler', COMPILER), ('runtime-gc', RUNTIME), ('stdlib', STDLIB),
                           ('tooling-ci', TOOLING)):
        if crate.startswith(prefixes):
            return area
    return None


def classify_pr(title: str, author: str, files: list[tuple[str, int]]) -> set[str]:
    labels = set()
    if author == 'app/dependabot':
        return {'area:deps', 'type:chore'}
    weight: dict[str, int] = {}
    for path, changes in files:
        area = area_of(path)
        if area:
            # +1 so a rename or a mode change still counts.
            weight[area] = weight.get(area, 0) + changes + 1
        crate = crate_of(path) or ''
        for prefix, platform in CRATE_PLATFORM:
            if crate.startswith(prefix):
                labels.add('platform:' + platform)
                break
    # A lockfile delta follows any dependency edit, so it decides only when nothing else does.
    if len(weight) > 1:
        weight.pop('deps', None)
    if weight:
        labels.add('area:' + max(sorted(weight), key=weight.get))
    match = re.match(r'([a-z]+)(\([^)]*\))?!?:', title.strip())
    if match and match.group(1) in TITLE_TYPE:
        scope = (match.group(2) or '').strip('()')
        labels.add('type:chore' if scope in CHORE_SCOPES else 'type:' + TITLE_TYPE[match.group(1)])
    return labels


def form_sections(body: str) -> dict[str, str]:
    sections = {}
    for chunk in re.split(r'^### ', body or '', flags=re.M)[1:]:
        heading, _, value = chunk.partition('\n')
        value = value.strip()
        sections[heading.strip()] = '' if value == '_No response_' else value
    return sections


def classify_issue(body: str, current: set[str]) -> tuple[set[str], set[str]]:
    sections = form_sections(body)
    add, remove = set(), set()
    chosen = sections.get('Target platform') or sections.get('Platforms it matters for') or ''
    for option in (x.strip() for x in chosen.split(',')):
        if option in FORM_PLATFORM:
            add.add('platform:' + FORM_PLATFORM[option])
        elif option == 'native (host OS)' and sections.get('Host OS (where you run perry)') in HOST_PLATFORM:
            add.add('platform:' + HOST_PLATFORM[sections['Host OS (where you run perry)']])
    if sections.get('Where does it go wrong?') == PERF_STAGE:
        add.add('type:perf')
        if 'type:bug' in current:
            remove.add('type:bug')
    return add - current, remove


def gh(*args: str) -> str:
    result = subprocess.run(['gh', *args], capture_output=True, text=True, timeout=120)
    if result.returncode != 0:
        raise SystemExit(f'gh {" ".join(args[:3])} failed ({result.returncode}): {result.stderr.strip()}')
    return result.stdout


def labelled_by_bot(repo: str, number: int) -> bool | None:
    """None when the item's events cannot be read; plan() skips it and the run fails."""
    try:
        actors = gh('api', '--paginate', f'repos/{repo}/issues/{number}/events',
                    '--jq', '.[] | select(.event == "labeled") | .actor.login')
    except SystemExit as error:
        print(f'#{number}: skipped, {error}', file=sys.stderr)
        return None
    return BOT in actors.split()


def plan(repo: str, hours: int) -> tuple[list[tuple[int, set[str], set[str]]], int]:
    """The label changes to make, and how many items were skipped because their events failed."""
    since = (dt.datetime.now(dt.timezone.utc) - dt.timedelta(hours=hours)).strftime('%Y-%m-%dT%H:%M:%SZ')
    search = f'created:>={since}'
    changes = []
    unread = 0
    issues = json.loads(gh('issue', 'list', '--repo', repo, '--state', 'open', '--limit', '200', '--search', search,
                           '--json', 'number,body,labels'))
    for issue in issues:
        current = {label['name'] for label in issue['labels']}
        if any(name.startswith('platform:') for name in current):
            continue
        add, remove = classify_issue(issue['body'], current)
        if add or remove:
            done = labelled_by_bot(repo, issue['number'])
            unread += done is None
            if done is False:
                changes.append((issue['number'], add, remove))
    prs = json.loads(gh('pr', 'list', '--repo', repo, '--state', 'open', '--limit', '200', '--search', search,
                        '--json', 'number,title,author,labels,files'))
    for pr in prs:
        current = {label['name'] for label in pr['labels']}
        if any(name.startswith('area:') for name in current):
            continue
        files = [(f['path'], f['additions'] + f['deletions']) for f in pr.get('files') or []]
        add = classify_pr(pr['title'], (pr.get('author') or {}).get('login', ''), files)
        if any(name.startswith('type:') for name in current):
            add = {name for name in add if not name.startswith('type:')}
        add -= current
        if add:
            done = labelled_by_bot(repo, pr['number'])
            unread += done is None
            if done is False:
                changes.append((pr['number'], add, set()))
    return changes, unread


def apply(repo: str, changes: list[tuple[int, set[str], set[str]]]) -> int:
    """Label every item, report each failure, and return how many failed."""
    failed = 0
    for number, add, remove in changes:
        # Remove first: only an added label marks the item as done, so a failure here is retried.
        try:
            for name in sorted(remove):
                gh('api', '-X', 'DELETE', f'repos/{repo}/issues/{number}/labels/{name.replace(":", "%3A")}')
            if add:
                flags = [arg for name in sorted(add) for arg in ('-f', f'labels[]={name}')]
                gh('api', '-X', 'POST', f'repos/{repo}/issues/{number}/labels', *flags)
        except SystemExit as error:
            print(f'#{number}: {error}', file=sys.stderr)
            failed += 1
    return failed


class Tests(unittest.TestCase):
    def test_pr_area_follows_the_most_changed_crate(self):
        files = [('crates/perry-runtime/src/gc/mark.rs', 120), ('crates/perry-codegen/src/expr.rs', 10)]
        self.assertEqual(classify_pr('fix(gc): keep roots', 'me', files), {'area:runtime-gc', 'type:bug'})

    def test_pr_ignores_docs_tests_and_changelog_for_area(self):
        files = [('docs/src/a.md', 500), ('changelog.d/1-x.md', 3), ('crates/perry-hir/src/lower.rs', 4)]
        self.assertEqual(classify_pr('feat: x', 'me', files), {'area:compiler', 'type:feature'})

    def test_pr_platform_comes_from_platform_crates(self):
        files = [('crates/perry-codegen-wasm/src/emit.rs', 5), ('crates/perry-ui-ios/src/app.rs', 5)]
        self.assertEqual(classify_pr('perf(wasm): y', 'me', files),
                         {'area:compiler', 'platform:wasm', 'platform:ios', 'type:perf'})

    def test_pr_cli_scripts_and_workflows_are_tooling(self):
        self.assertEqual(classify_pr('ci: z', 'me', [('crates/perry/src/main.rs', 1), ('.github/workflows/test.yml', 9)]),
                         {'area:tooling-ci', 'type:chore'})

    def test_pr_unknown_prefix_and_unknown_paths_add_nothing(self):
        self.assertEqual(classify_pr('Update README', 'me', [('README.md', 3)]), set())

    def test_ci_and_deps_scoped_fixes_are_chores(self):
        self.assertEqual(classify_pr('fix(ci): drop a duplicate', 'me', [('.github/workflows/test.yml', 2)]),
                         {'area:tooling-ci', 'type:chore'})
        self.assertEqual(classify_pr('deps: bump taffy', 'me', [('Cargo.lock', 2)]), {'area:deps', 'type:chore'})

    def test_lockfile_churn_does_not_outvote_the_crate(self):
        files = [('Cargo.lock', 400), ('crates/perry-stdlib/src/zlib.rs', 20)]
        self.assertEqual(classify_pr('feat(zlib): brotli', 'me', files), {'area:stdlib', 'type:feature'})

    def test_dependabot_is_a_dependency_chore(self):
        self.assertEqual(classify_pr('build(deps): bump zip', 'app/dependabot', [('Cargo.lock', 9)]),
                         {'area:deps', 'type:chore'})

    def test_issue_targets_map_to_platform_labels(self):
        body = ('### Host OS (where you run perry)\n\nmacOS\n\n'
                '### Target platform\n\nnative (host OS), ios-simulator, web, tvos\n\n### Device\n\n_No response_\n')
        self.assertEqual(classify_issue(body, {'type:bug'}),
                         ({'platform:macos', 'platform:ios', 'platform:wasm'}, set()))

    def test_issue_performance_stage_swaps_bug_for_perf(self):
        body = f'### Where does it go wrong?\n\n{PERF_STAGE}\n'
        self.assertEqual(classify_issue(body, {'type:bug'}), ({'type:perf'}, {'type:bug'}))

    def test_plan_leaves_items_the_bot_already_labelled(self):
        perf = {'number': 1, 'body': f'### Where does it go wrong?\n\n{PERF_STAGE}\n', 'labels': [{'name': 'type:bug'}]}
        for actors, expected in (('someone\n', [(1, {'type:perf'}, {'type:bug'})]), (f'someone\n{BOT}\n', [])):
            def fake_gh(*args, actors=actors):
                if args[:2] == ('issue', 'list'):
                    return json.dumps([perf])
                return '[]' if args[:2] == ('pr', 'list') else actors
            with mock.patch.object(sys.modules[__name__], 'gh', fake_gh):
                self.assertEqual(plan('o/r', 24), (expected, 0))

    def test_apply_keeps_going_after_a_failed_item(self):
        calls = []
        def fake_gh(*args):
            calls.append(args)
            if 'repos/o/r/issues/1/labels' in args:
                raise SystemExit('gh api failed (1): HTTP 404')
            return ''
        with mock.patch.object(sys.modules[__name__], 'gh', fake_gh), mock.patch('sys.stderr'):
            self.assertEqual(apply('o/r', [(1, {'type:bug'}, set()), (2, {'type:bug'}, set())]), 1)
        self.assertIn('repos/o/r/issues/2/labels', calls[-1])

    def test_apply_removes_before_it_adds(self):
        calls = []
        with mock.patch.object(sys.modules[__name__], 'gh', lambda *args: calls.append(args[2]) or ''):
            self.assertEqual(apply('o/r', [(1, {'type:perf'}, {'type:bug'})]), 0)
        self.assertEqual(calls, ['DELETE', 'POST'])

    def test_plan_skips_only_the_item_whose_events_fail(self):
        issues = [{'number': n, 'body': f'### Where does it go wrong?\n\n{PERF_STAGE}\n', 'labels': []} for n in (1, 2)]
        def fake_gh(*args):
            if args[:2] == ('issue', 'list'):
                return json.dumps(issues)
            if args[:2] == ('pr', 'list'):
                return '[]'
            if 'repos/o/r/issues/1/events' in args:
                raise SystemExit('gh api failed (1): HTTP 502')
            return 'someone\n'
        with mock.patch.object(sys.modules[__name__], 'gh', fake_gh), mock.patch('sys.stderr'):
            self.assertEqual(plan('o/r', 24), ([(2, {'type:perf'}, set())], 1))

    def test_feature_all_platforms_and_free_text_issues_add_nothing(self):
        self.assertEqual(classify_issue('### Platforms it matters for\n\nAll platforms\n', set()), (set(), set()))
        self.assertEqual(classify_issue('it crashes on my machine', set()), (set(), set()))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--apply', action='store_true', help='write the labels instead of printing them')
    parser.add_argument('--hours', type=int, default=24, help='only items created in the last N hours')
    parser.add_argument('--repo', default='PerryTS/perry')
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test:
        result = unittest.TextTestRunner(verbosity=1).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests))
        return 0 if result.wasSuccessful() else 1
    changes, unread = plan(args.repo, args.hours)
    for number, add, remove in changes:
        print(f'#{number}: +{sorted(add)} -{sorted(remove)}')
    print(f'{len(changes)} item(s) to label' + ('' if args.apply else ' (dry run)'))
    failed = apply(args.repo, changes) if args.apply else 0
    if unread:
        print(f'{unread} item(s) skipped: their label events could not be read', file=sys.stderr)
    return 1 if failed or unread else 0


if __name__ == '__main__':
    sys.exit(main())
