#!/usr/bin/env python3
"""Require native header validators to use value::addr_class.

All direct generation/malloc ownership calls are inventoried, including code
outside string modules. Exceptions carry a purpose and an exact per-function reference
count (including aliases and imported function names); an extra call in an allowlisted file still fails. GC/arena internals are
explicitly allowlisted owners rather than silently omitted from the scan.
"""
import argparse
from collections import Counter
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
CALL = re.compile(r'\b(classify_heap_generation|gc_malloc_header_is_tracked)\b')

def inventory(root):
    hits = []
    for crate in sorted((root / 'crates').glob('perry-*')):
        if crate.name not in ('perry-runtime', 'perry-stdlib') and not crate.name.startswith('perry-ext-'):
            continue
        for path in sorted((crate / 'src').rglob('*.rs')):
            if any(part.startswith('.') for part in path.relative_to(root).parts):
                continue
            function = '<module>'
            for n, line in enumerate(path.read_text().splitlines(), 1):
                code = line.split('//', 1)[0]
                definition = re.search(r'\bfn\s+(\w+)', code)
                if definition: function = definition[1]
                for match in CALL.finditer(code):
                    hits.append((path.relative_to(root).as_posix(), function, match[1], n))
    return hits

def check(root):
    entries = []
    for line in (root / 'scripts/string_validator_allowlist.txt').read_text().splitlines():
        if not line.strip() or line.startswith('#'):
            continue
        path, function, symbol, count, reason = [part.strip() for part in line.split('|', 4)]
        assert reason and symbol in ('*', 'classify_heap_generation', 'gc_malloc_header_is_tracked')
        assert count == '*' or int(count) > 0
        # Wildcards belong only to the metadata owners, never native validators.
        if count == '*':
            assert path in ('crates/perry-runtime/src/gc/', 'crates/perry-runtime/src/arena/')
        entries.append((path, function, symbol, count))
    counts = Counter()
    failures = []
    for path, function, symbol, n in inventory(root):
        entry = next((e for e in entries if (path == e[0] or (e[0].endswith('/') and path.startswith(e[0]))) and e[1] in ('*', function) and e[2] in ('*', symbol)), None)
        if entry is None:
            failures.append(f'{path}:{n}: direct {symbol}; use the canonical tracked/string header classifier')
        else:
            counts[entry] += 1
    for entry in entries:
        if entry[3] != '*' and counts[entry] != int(entry[3]):
            failures.append(f'{entry[0]} {entry[1]} {entry[2]}: {counts[entry]} references, justified count {entry[3]} (new call or stale exception)')
    return failures

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=ROOT)
    args = parser.parse_args()
    failures = check(args.root)
    if failures:
        print('\n'.join(failures))
        return 1
    print('Native string-validator inventory passed.')
    return 0

if __name__ == '__main__':
    raise SystemExit(main())
