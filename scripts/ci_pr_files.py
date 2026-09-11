#!/usr/bin/env python3
"""Emit a complete, stable PR file list or fail without printing partial scope.

gh pr view --json files returns at most100 paths. Large benchmark-evidence PRs
can put every source/test path beyond that page, making scoped CI run nothing.
REST pagination is required, but its3000-file server cap must also fail closed:
compare the unique returned filenames with the PR's changed_files count, and
verify head/base/count stayed unchanged while fetching the pages.
API limit: https://docs.github.com/en/rest/pulls/pulls#list-pull-requests-files
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


def changed_files(repository, number, expected_head, api=gh_api):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('repository must be owner/name')
    if type(number) is not int or number <= 0 or not re.fullmatch(r'[0-9a-f]{40}', expected_head):
        raise ValueError('positive PR number and full expected head SHA required')
    endpoint = f'repos/{repository}/pulls/{number}'
    before = snapshot(api(endpoint), expected_head)
    pages = api(endpoint + '/files?per_page=100', '--paginate', '--slurp')
    if not isinstance(pages, list) or not pages:
        raise ValueError('PR files response must contain JSON array pages')
    paths = []
    for page in pages:
        if not isinstance(page, list):
            raise ValueError('PR files page is not an array')
        for entry in page:
            path = entry.get('filename') if isinstance(entry, dict) else None
            if not isinstance(path, str) or not path or path != path.strip() or any(c in path for c in '\r\n\x00'):
                raise ValueError('invalid filename for line-oriented CI scope')
            paths.append(path)
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
        self.assertEqual(workflow.count(command), 3)
        self.assertEqual(workflow.count('--expected-head "$PR_HEAD_SHA"'), 3)
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


def main():
    if sys.argv[1:] == ['--self-test']:
        result = unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromTestCase(PaginationTests))
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
