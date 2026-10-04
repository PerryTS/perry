"""Reject an unchanged archive or a runtime suite that never ran new tests."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1048576), b''):
            h.update(chunk)
    return h.hexdigest()


def verify(log, archive, previous, pages):
    text = log.read_text()
    if 'Compiling perry-runtime v' not in text:
        raise ValueError('runtime test crate was not rebuilt')
    tests = ['only_complete_owned_pages_are_eligible',
             'kernel_releases_interior_pages_without_touching_either_neighbor']
    if pages:
        tests += ['compact_metadata_preserves_wide_dirty_work_and_epoch',
                  'page_chunks_match_independent_map_through_boundary_reuse',
                  'last_page_removal_releases_storage_and_reuse_is_clean']
    for test in tests:
        if not re.search(r'^test .*::' + re.escape(test) + r' \.\.\. ok$', text, re.M):
            raise ValueError(f'new production-helper test did not pass: {test}')
    verdicts = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', text)
    if not verdicts or int(verdicts[-1][0]) < 4818 or int(verdicts[-1][1]) != 0:
        raise ValueError('missing successful complete runtime suite')
    current, old = sha(archive), sha(previous)
    if current == old:
        raise ValueError('runtime archive unchanged from preceding source revision')
    symbols = subprocess.check_output(['nm', '--defined-only', str(archive)], stderr=subprocess.DEVNULL)
    if b'decommit' not in symbols:
        raise ValueError('production archive lacks page-release module witness')
    return dict(runtime_suite_counts=list(map(int, verdicts[-1])), required_tests=tests,
                test_log_sha256=sha(log), archive_sha256=current, preceding_archive_sha256=old,
                decommit_symbol_witness=True, symbol_listing_sha256=hashlib.sha256(symbols).hexdigest())


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('test_log', type=Path)
    parser.add_argument('archive', type=Path)
    parser.add_argument('previous_archive', type=Path)
    parser.add_argument('receipt', type=Path)
    parser.add_argument('--pages', action='store_true')
    args = parser.parse_args()
    result = verify(args.test_log, args.archive, args.previous_archive, args.pages)
    args.receipt.write_text(json.dumps(result, indent=2) + '\n')
    print('Verified new runtime tests, changed archive and production decommit symbol.')
