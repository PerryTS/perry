#!/usr/bin/env python3
"""Emit a complete, stable PR file list or fail without printing partial scope.

gh pr view --json files returns at most100 paths. Large benchmark-evidence PRs
can put every source/test path beyond that page, making scoped CI run nothing.
REST pagination is checked against changed_files for PRs up to the3000-file
server cap. Larger PRs compare complete immutable merge-base/head Git trees,
retaining both rename endpoints. Partial recursive payloads are discarded and
replaced by complete child-tree traversal, or fail closed. Both routes
verify head/base/count stayed unchanged before returning any scope.
API limits: https://docs.github.com/en/rest/pulls/pulls#list-pull-requests-files
and https://docs.github.com/en/rest/git/trees#get-a-tree
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import unittest


def gh_api(endpoint, *flags):
    result = subprocess.run(['gh', 'api', endpoint, *flags], check=True,
                            capture_output=True, text=True, timeout=180)
    return json.loads(result.stdout)


def snapshot(metadata, expected_head):
    if not isinstance(metadata, dict):
        raise ValueError('PR metadata is not an object')
    head = metadata.get('head', {}).get('sha')
    base = metadata.get('base', {}).get('sha')
    count = metadata.get('changed_files')
    if head != expected_head or not isinstance(base, str) or not re.fullmatch(r'[0-9a-f]{40}', base):
        raise ValueError('PR head does not match this run or base SHA is invalid')
    if type(count) is not int or count < 0:
        raise ValueError('PR changed_files count is invalid')
    return head, base, count



def filename(path):
    if not isinstance(path, str) or not path or path != path.strip() or any(c in path for c in '\r\n\x00'):
        raise ValueError('invalid filename for line-oriented CI scope')
    return path


def oid(value):
    return isinstance(value, str) and re.fullmatch(r'[0-9a-f]{40}', value) is not None


def tree_files(repository, commit, api, cache=None):
    obj = api(f'repos/{repository}/git/commits/{commit}')
    tree = obj.get('tree') if isinstance(obj, dict) else None
    if (not isinstance(obj, dict) or obj.get('sha') != commit
            or not isinstance(tree, dict) or not oid(tree.get('sha'))):
        raise ValueError('incomplete or unbound Git commit response')
    tree_sha = tree['sha']
    return tree_inventory(repository, tree_sha, api, {} if cache is None else cache, set())


def tree_inventory(repository, tree_sha, api, cache, active):
    if tree_sha in cache:
        return cache[tree_sha]
    if tree_sha in active:
        raise ValueError('cyclic Git tree response')
    active = active | {tree_sha}
    obj = api(f'repos/{repository}/git/trees/{tree_sha}?recursive=1')
    if (not isinstance(obj, dict) or obj.get('sha') != tree_sha
            or type(obj.get('truncated')) is not bool or not isinstance(obj.get('tree'), list)):
        raise ValueError('incomplete Git tree response')
    recursive = not obj['truncated']
    if not recursive:
        # Discard the partial array; complete child trees establish scope.
        obj = api(f'repos/{repository}/git/trees/{tree_sha}')
        if (not isinstance(obj, dict) or obj.get('sha') != tree_sha
                or obj.get('truncated') is not False or not isinstance(obj.get('tree'), list)):
            raise ValueError('incomplete or truncated nonrecursive Git tree response')
    files, seen = {}, set()
    for entry in obj['tree']:
        if not isinstance(entry, dict):
            raise ValueError('invalid Git tree entry')
        path = filename(entry.get('path'))
        if (path.startswith('/') or any(part in ('', '.', '..') for part in path.split('/'))
                or (not recursive and '/' in path)):
            raise ValueError('invalid repository-relative Git tree path')
        kind, mode, sha = entry.get('type'), entry.get('mode'), entry.get('sha')
        if (path in seen or kind not in ('blob', 'tree', 'commit') or not oid(sha)
                or not isinstance(mode, str) or not re.fullmatch(r'[0-7]{6}', mode)):
            raise ValueError('duplicate or malformed Git tree entry')
        seen.add(path)
        if kind != 'tree':
            files[path] = (kind, mode, sha)
        elif not recursive:
            for child, value in tree_inventory(repository, sha, api, cache, active).items():
                files[path + '/' + child] = value
    cache[tree_sha] = files
    return files


def large_scope(repository, snapshot_before, api):
    head, base, count = snapshot_before
    comparison = api(f'repos/{repository}/compare/{base}...{head}')
    base_obj = comparison.get('base_commit') if isinstance(comparison, dict) else None
    merge_obj = comparison.get('merge_base_commit') if isinstance(comparison, dict) else None
    if (not isinstance(base_obj, dict) or base_obj.get('sha') != base
            or not isinstance(merge_obj, dict) or not oid(merge_obj.get('sha'))):
        raise ValueError('incomplete or unbound comparison merge base')
    cache = {}
    before = tree_files(repository, merge_obj['sha'], api, cache)
    after = tree_files(repository, head, api, cache)
    # Rename classification is unnecessary for scope: retain both endpoints.
    paths = sorted(path for path in before.keys() | after.keys()
                   if before.get(path) != after.get(path))
    if not count <= len(paths) <= 2 * count:
        raise ValueError('incomplete Git tree scope relative to PR changed_files count')
    return paths


def changed_files(repository, number, expected_head, api=gh_api):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('repository must be owner/name')
    if type(number) is not int or number <= 0 or not re.fullmatch(r'[0-9a-f]{40}', expected_head):
        raise ValueError('positive PR number and full expected head SHA required')
    endpoint = f'repos/{repository}/pulls/{number}'
    before = snapshot(api(endpoint), expected_head)
    if before[2] > 3000:
        paths = large_scope(repository, before, api)
    else:
        pages = api(endpoint + '/files?per_page=100', '--paginate', '--slurp')
        if not isinstance(pages, list) or not pages:
            raise ValueError('PR files response must contain JSON array pages')
        paths = []
        for page in pages:
            if not isinstance(page, list):
                raise ValueError('PR files page is not an array')
            for entry in page:
                path = entry.get('filename') if isinstance(entry, dict) else None
                paths.append(filename(path))
        if len(set(paths)) != len(paths) or len(paths) != before[2]:
            raise ValueError(f'incomplete PR file list: {len(paths)} paths / {len(set(paths))} unique, expected {before[2]}; pagination/cap or PR race')
    if snapshot(api(endpoint), expected_head) != before:
        raise ValueError('PR head/base/file count changed while listing files')
    return paths


class PaginationTests(unittest.TestCase):
    head = 'a' * 40
    metadata = {'head': {'sha': 'a' * 40}, 'base': {'sha': 'b' * 40}, 'changed_files': 251}

    def test_workflow_consumers_use_verified_scope(self):
        # Binding tripwire in addition to the live pagination tests below.
        workflow = (Path(__file__).resolve().parents[1] / '.github/workflows/test.yml').read_text()
        command = 'python3 scripts/ci_pr_files.py "$REPOSITORY" "$PR_NUMBER"'
        self.assertEqual(workflow.count(command), 4)
        self.assertEqual(workflow.count('--expected-head "$PR_HEAD_SHA"'), 5)
        self.assertNotIn('--json files', workflow)

    def fixture(self, pages=None, before=None, after=None):
        names = [f'benchmarks/results/{n:04}.json' for n in range(250)]
        names.append('crates/perry/tests/child_output_late_iterator.rs')
        entries = [{'filename': name} for name in names]
        if pages is None:
            pages = [entries[:100], entries[100:200], entries[200:]]
        responses = [self.metadata if before is None else before, pages,
                     self.metadata if after is None else after]
        calls = []
        def api(endpoint, *flags):
            calls.append((endpoint, flags))
            return responses.pop(0)
        return names, api, calls

    def test_late_source_survives_all_pages(self):
        names, api, calls = self.fixture()
        self.assertEqual(changed_files('PerryTS/perry', 10047, self.head, api), names)
        self.assertEqual(calls[1], ('repos/PerryTS/perry/pulls/10047/files?per_page=100', ('--paginate', '--slurp')))
        self.assertEqual(len(calls), 3)

    def test_first_page_only_fails(self):
        _, api, _ = self.fixture(pages=[[{'filename': str(n)} for n in range(100)]])
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            changed_files('PerryTS/perry', 10047, self.head, api)

    def test_server_cap_fails(self):
        meta = {**self.metadata, 'changed_files': 3001}
        _, api, _ = self.fixture(pages=[[{'filename': str(n)} for n in range(3000)]], before=meta)
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            changed_files('PerryTS/perry', 10047, self.head, api)

    def test_duplicate_page_fails_even_when_count_matches(self):
        _, api, _ = self.fixture(pages=[[{'filename': 'same'}] * 251])
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            changed_files('PerryTS/perry', 10047, self.head, api)

    def test_changes_during_pagination_fail(self):
        for update in [{'head': {'sha': 'c' * 40}}, {'base': {'sha': 'c' * 40}}, {'changed_files': 252}]:
            with self.subTest(update=update):
                _, api, _ = self.fixture(after={**self.metadata, **update})
                with self.assertRaises(ValueError):
                    changed_files('PerryTS/perry', 10047, self.head, api)

    def test_stale_run_fails_before_listing(self):
        _, api, calls = self.fixture(before={**self.metadata, 'head': {'sha': 'c' * 40}})
        with self.assertRaises(ValueError):
            changed_files('PerryTS/perry', 10047, self.head, api)
        self.assertEqual(len(calls), 1)

    def test_malformed_page_or_line_protocol_fails(self):
        for pages in [{}, [None], [[None]], [[{'filename': 'crates/x\nother'}]]]:
            with self.subTest(pages=pages):
                _, api, _ = self.fixture(pages=pages)
                with self.assertRaises(ValueError):
                    changed_files('PerryTS/perry', 10047, self.head, api)

    def test_api_error_is_not_empty_scope(self):
        def broken(*args):
            raise subprocess.CalledProcessError(1, ['gh', 'api'])
        with self.assertRaises(subprocess.CalledProcessError):
            changed_files('PerryTS/perry', 10047, self.head, broken)

    def test_empty_pr_count_is_checked(self):
        meta = {**self.metadata, 'changed_files': 0}
        _, api, _ = self.fixture(pages=[[]], before=meta, after=meta)
        self.assertEqual(changed_files('PerryTS/perry', 10047, self.head, api), [])



class LargeScopeTests(unittest.TestCase):
    def fixture(self, *, after=None):
        endpoint = 'repos/PerryTS/perry/pulls/10047'
        head, base, merge = 'a' * 40, 'b' * 40, 'c' * 40
        old_tree, new_tree = 'd' * 40, 'e' * 40
        metadata = {'head': {'sha': head}, 'base': {'sha': base}, 'changed_files': 3001}
        old = [{'path': f'benchmarks/before/{n:04}.json', 'type': 'blob',
                'mode': '100644', 'sha': '1' * 40} for n in range(3000)]
        new = [{**entry, 'path': entry['path'].replace('/before/', '/after/')}
               for entry in old]
        old.append({'path': 'crates/perry/src/source.rs', 'type': 'blob',
                    'mode': '100644', 'sha': '3' * 40})
        new.append({**old[-1], 'sha': '4' * 40})
        objects = {
            f'repos/PerryTS/perry/compare/{base}...{head}': {
                'base_commit': {'sha': base}, 'merge_base_commit': {'sha': merge}},
            f'repos/PerryTS/perry/git/commits/{merge}': {'sha': merge, 'tree': {'sha': old_tree}},
            f'repos/PerryTS/perry/git/commits/{head}': {'sha': head, 'tree': {'sha': new_tree}},
            f'repos/PerryTS/perry/git/trees/{old_tree}?recursive=1': {
                'sha': old_tree, 'truncated': False, 'tree': old},
            f'repos/PerryTS/perry/git/trees/{new_tree}?recursive=1': {
                'sha': new_tree, 'truncated': False, 'tree': new},
        }
        calls = []
        def api(path, *flags):
            calls.append(path)
            if path == endpoint:
                return metadata if calls.count(endpoint) == 1 else (after or metadata)
            if path == endpoint + '/files?per_page=100':
                return [[{'filename': f'benchmarks/after/{n:04}.json'} for n in range(3000)]]
            if '/git/trees/' in path and '?' not in path and path not in objects:
                return {'sha': path.rsplit('/', 1)[-1], 'truncated': True, 'tree': []}
            return objects[path]
        names = sorted({entry['path'] for entry in old + new})
        return api, calls, objects, names

    def test_complete_trees_retain_source_after_three_thousand_renames(self):
        api, calls, objects, names = self.fixture()
        before = json.dumps(objects, sort_keys=True)
        self.assertEqual(changed_files('PerryTS/perry', 10047, 'a' * 40, api), names)
        self.assertEqual(len(names), 6001)
        self.assertIn('crates/perry/src/source.rs', names)
        self.assertFalse(any('/files?' in path for path in calls))
        self.assertTrue(any('/git/commits/' + 'c' * 40 in path for path in calls))
        self.assertEqual(json.dumps(objects, sort_keys=True), before)

    def test_truncated_or_malformed_trees_cannot_emit_scope(self):
        for change in ['truncated', 'missing_flag', 'duplicate', 'bad_path', 'bad_sha', 'too_few']:
            with self.subTest(change=change):
                api, calls, objects, _ = self.fixture()
                tree = objects['repos/PerryTS/perry/git/trees/' + 'e' * 40 + '?recursive=1']
                if change == 'truncated': tree['truncated'] = True
                elif change == 'missing_flag': del tree['truncated']
                elif change == 'duplicate': tree['tree'].append(tree['tree'][0])
                elif change == 'bad_path': tree['tree'][0]['path'] = 'source\nother'
                elif change == 'bad_sha': tree['sha'] = '0' * 40
                else:
                    tree['tree'].clear()
                    objects['repos/PerryTS/perry/git/trees/' + 'd' * 40 + '?recursive=1']['tree'].clear()
                before = json.dumps(objects, sort_keys=True)
                with self.assertRaises(ValueError):
                    changed_files('PerryTS/perry', 10047, 'a' * 40, api)
                self.assertTrue(any('/git/trees/' in path for path in calls))
                self.assertEqual(json.dumps(objects, sort_keys=True), before)

    def test_large_scope_still_rejects_a_pr_race(self):
        after = {'head': {'sha': 'a' * 40}, 'base': {'sha': 'f' * 40}, 'changed_files': 3001}
        api, calls, _, _ = self.fixture(after=after)
        with self.assertRaisesRegex(ValueError, 'changed while'):
            changed_files('PerryTS/perry', 10047, 'a' * 40, api)
        self.assertEqual(calls[-1], 'repos/PerryTS/perry/pulls/10047')

    def test_partial_recursive_arrays_are_replaced_by_complete_child_trees(self):
        api, calls, objects, names = self.fixture()
        for root_sha, child_sha in [('d' * 40, '8' * 40), ('e' * 40, '9' * 40)]:
            endpoint = 'repos/PerryTS/perry/git/trees/' + root_sha
            payload = objects[endpoint + '?recursive=1']
            entries = payload['tree']
            payload.update(truncated=True, tree=[{'path': 'discarded-partial-array'}])
            objects[endpoint] = {'sha': root_sha, 'truncated': False,
                                 'tree': [{'path': 'payload', 'type': 'tree',
                                           'mode': '040000', 'sha': child_sha}]}
            objects['repos/PerryTS/perry/git/trees/' + child_sha + '?recursive=1'] = {
                'sha': child_sha, 'truncated': False, 'tree': entries}
        self.assertEqual(changed_files('PerryTS/perry', 10047, 'a' * 40, api),
                         ['payload/' + name for name in names])
        self.assertIn('repos/PerryTS/perry/git/trees/' + 'd' * 40, calls)

    def test_large_deletion_only_scope_is_complete(self):
        api, _, objects, _ = self.fixture()
        old = objects['repos/PerryTS/perry/git/trees/' + 'd' * 40 + '?recursive=1']['tree']
        objects['repos/PerryTS/perry/git/trees/' + 'e' * 40 + '?recursive=1']['tree'].clear()
        self.assertEqual(changed_files('PerryTS/perry', 10047, 'a' * 40, api),
                         sorted(entry['path'] for entry in old))

    def test_mode_only_changes_are_retained(self):
        api, _, objects, names = self.fixture()
        entry = objects['repos/PerryTS/perry/git/trees/' + 'e' * 40 + '?recursive=1']['tree'][-1]
        entry.update(sha='3' * 40, mode='100755')
        self.assertEqual(changed_files('PerryTS/perry', 10047, 'a' * 40, api), names)


def main():
    if sys.argv[1:] == ['--self-test']:
        result = unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__]))
        return 0 if result.wasSuccessful() else 1
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('repository')
    parser.add_argument('number', type=int)
    parser.add_argument('--expected-head', required=True)
    args = parser.parse_args()
    try:
        paths = changed_files(args.repository, args.number, args.expected_head)
    except (ValueError, subprocess.SubprocessError) as error:
        print(f'::error::Cannot establish complete PR file scope: {error}', file=sys.stderr)
        return 1
    if paths:
        print('\n'.join(paths))
    return 0


if __name__ == '__main__':
    sys.exit(main())
