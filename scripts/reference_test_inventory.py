#!/usr/bin/env python3
"""Require the JavaScript-semantic dispatch paths to test references canonically.

Since honest pointers (#12529) `POINTER_TAG` names only a GC-headed heap
object; a native handle (Headers, Blob, Request/Response, Web Streams,
sockets, ...) carries `HANDLE_TAG` instead. In JavaScript a handle is still an
object, so a ToPrimitive / ToNumber / ToString ladder, an equality or a
serializer that branches on `is_pointer()` sends it into the "plain number"
arm (#12540: `1 / headers` was typeof "object", `headers + 1` was "NaN").

In the modules listed in SEMANTIC_PATHS every raw pointer-tag test
(`.is_pointer()`, a compare against `POINTER_TAG` or its 0x7FFD top half) must
either become `JSValue::is_reference()` / `value::is_reference_bits()` or be
justified in scripts/reference_test_allowlist.txt with an exact per-function
count and the reason a header-only test is right there (the next step reads a
GC header, proves a closure, ...). An extra raw test in a listed function,
or a stale count, fails.

Generated code has the same hazard for numbers: a one-sided signed compare
against the tag-band floor (`icmp slt bits, 0x7FF9<<48`) admits HANDLE_TAG
(0xFFF9, a negative NaN) as a plain number, so `handle == handle` answered
`fcmp oeq` = false. perry-codegen must use the shared sign-closed
`emit_plain_number_bits_test`; any `icmp_slt` against the band floor fails.
"""
import argparse
from collections import Counter
from pathlib import Path
import re
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ALLOWLIST = 'scripts/reference_test_allowlist.txt'

# JavaScript-semantic dispatch: coercion, equality, typeof, inspection,
# serialization, instanceof and structured clone.
SEMANTIC_PATHS = (
    'crates/perry-runtime/src/value/',
    'crates/perry-runtime/src/builtins/arithmetic.rs',
    'crates/perry-runtime/src/builtins/numbers.rs',
    'crates/perry-runtime/src/builtins/formatting.rs',
    'crates/perry-runtime/src/builtins/formatting/',
    'crates/perry-runtime/src/builtins/console.rs',
    'crates/perry-runtime/src/object/instanceof.rs',
    'crates/perry-runtime/src/object/instanceof/',
    'crates/perry-runtime/src/symbol/iterator.rs',
    'crates/perry-runtime/src/thread/clone_write.rs',
)

RAW = re.compile(
    r'\.is_pointer\(\)'
    r'|[=!]=\s*(?:[\w:]*::)?POINTER_TAG\b(?!_)'
    r'|\bPOINTER_TAG\s*[=!]='
    r'|[=!]=\s*0x7[Ff][Ff][Dd]\b|\b0x7[Ff][Ff][Dd]\s*[=!]='
)
FN = re.compile(r'\bfn\s+(\w+)')
# `icmp_slt(<ty>, <bits>, <tag-band floor>)`, possibly over several lines.
ONE_SIDED_NUMBER = re.compile(
    r'icmp_slt\(\s*I64\s*,[^;]*?(?:SHORT_STRING_TAG\b|"9221401712017801216"|LOWEST_NANBOX_TAG)',
    re.S)


def is_test_file(rel):
    name = rel.rsplit('/', 1)[-1]
    return name.startswith('tests') or name.endswith('_tests.rs') or '/tests/' in rel


def inventory(root):
    hits = []
    for prefix in SEMANTIC_PATHS:
        base = root / prefix
        paths = [base] if base.suffix == '.rs' else sorted(base.rglob('*.rs')) if base.is_dir() else []
        for path in paths:
            if not path.is_file():
                continue
            rel = path.relative_to(root).as_posix()
            if is_test_file(rel):
                continue
            function = '<module>'
            skip_depth = None  # brace depth while inside a #[cfg(test)] item
            pending_test_item = False
            depth = 0
            for n, line in enumerate(path.read_text().splitlines(), 1):
                code = line.split('//', 1)[0]
                stripped = code.strip()
                opened, closed = code.count('{'), code.count('}')
                if skip_depth is not None:
                    depth += opened - closed
                    if depth <= skip_depth:
                        skip_depth = None
                    continue
                if stripped.startswith('#[cfg(test)]') or stripped.startswith('#[cfg(all(test'):
                    pending_test_item = True
                    continue
                if pending_test_item and stripped and not stripped.startswith('#['):
                    pending_test_item = False
                    if opened > closed:  # a test-only block item: skip its body
                        skip_depth = depth
                        depth += opened - closed
                        continue
                    if stripped.endswith(';') or opened == closed:
                        continue  # `mod tests;` or a one-line test item
                depth += opened - closed
                definition = FN.search(code)
                if definition:
                    function = definition[1]
                for _ in RAW.finditer(code):
                    hits.append((rel, function, n))
    return hits


def codegen_one_sided_number_tests(root):
    hits = []
    for path in sorted((root / 'crates/perry-codegen/src').rglob('*.rs')):
        rel = path.relative_to(root).as_posix()
        if is_test_file(rel):
            continue
        text = '\n'.join(line.split('//', 1)[0] for line in path.read_text().splitlines())
        for match in ONE_SIDED_NUMBER.finditer(text):
            hits.append(f'{rel}:{text.count(chr(10), 0, match.start()) + 1}')
    return hits


def load_allowlist(root):
    entries = {}
    for line in (root / ALLOWLIST).read_text().splitlines():
        if not line.strip() or line.startswith('#'):
            continue
        path, function, count, reason = [part.strip() for part in line.split('|', 3)]
        assert reason, f'{path} {function}: an exception needs its reason'
        assert int(count) > 0, f'{path} {function}: count must be positive'
        assert (path, function) not in entries, f'duplicate entry {path} {function}'
        entries[(path, function)] = int(count)
    return entries


def check(root):
    entries = load_allowlist(root)
    counts = Counter()
    failures = []
    for path, function, n in inventory(root):
        if (path, function) not in entries:
            failures.append(
                f'{path}:{n}: raw pointer-tag test in {function}; a JavaScript-semantic '
                'branch uses JSValue::is_reference() / value::is_reference_bits() (a native '
                'handle is an object too, #12540). If the next step reads a GC header, '
                f'justify it in {ALLOWLIST}')
        counts[(path, function)] += 1
    for hit in codegen_one_sided_number_tests(root):
        failures.append(
            f'{hit}: one-sided signed compare against the tag-band floor admits HANDLE_TAG '
            'as a number; use crate::codegen::emit_plain_number_bits_test (#12540)')
    for (path, function), want in entries.items():
        if counts[(path, function)] != want:
            failures.append(
                f'{path} {function}: {counts[(path, function)]} raw pointer-tag tests, '
                f'justified count {want} (new test or stale exception)')
    return failures


def self_test():
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        src = root / 'crates/perry-runtime/src/value'
        src.mkdir(parents=True)
        (root / 'scripts').mkdir()
        (src / 'coerce.rs').write_text(
            'fn header_reader(v: JSValue) {\n    if v.is_pointer() { read_header(v) }\n}\n'
            'fn ladder(v: JSValue) {\n    if v.is_reference() { to_primitive(v) }\n}\n')
        (root / ALLOWLIST).write_text(
            'crates/perry-runtime/src/value/coerce.rs | header_reader | 1 | reads the header\n')
        assert check(root) == [], check(root)
        # A new raw test in a semantic ladder fails.
        (src / 'coerce.rs').write_text(
            (src / 'coerce.rs').read_text()
            + 'fn other(bits: u64) {\n    if bits & TAG_MASK == POINTER_TAG { x() }\n}\n')
        failures = check(root)
        assert len(failures) == 1 and 'other' in failures[0], failures
        # An extra raw test in an allowlisted function fails on the count.
        (src / 'coerce.rs').write_text(
            'fn header_reader(v: JSValue) {\n    if v.is_pointer() && w.is_pointer() {}\n}\n')
        failures = check(root)
        assert len(failures) == 1 and 'justified count 1' in failures[0], failures
        # Comments and inline test modules are not code under audit.
        (src / 'coerce.rs').write_text(
            'fn header_reader(v: JSValue) {\n    if v.is_pointer() {} // v.is_pointer()\n}\n'
            '#[cfg(test)]\nmod tests { fn t() { v.is_pointer(); } }\n')
        assert check(root) == [], check(root)
        # Codegen: a one-sided number test against the tag band fails.
        cg = root / 'crates/perry-codegen/src'
        cg.mkdir(parents=True)
        (cg / 'cmp.rs').write_text(
            'fn f(blk: &mut B) {\n    blk.icmp_slt(\n        I64,\n        &bits,\n'
            '        &i64_literal(SHORT_STRING_TAG),\n    );\n}\n')
        failures = check(root)
        assert len(failures) == 1 and 'cmp.rs:2' in failures[0], failures
        (cg / 'cmp.rs').write_text(
            'fn f(blk: &mut B) {\n    emit_plain_number_bits_test(blk, &bits);\n'
            '    blk.icmp_slt(I64, &bits, "0");\n}\n')
        assert check(root) == [], check(root)
    print('reference-test inventory self-test passed.')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--self-test', action='store_true')
    parser.add_argument('--list', action='store_true', help='print every raw test site')
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.list:
        for path, function, n in inventory(args.root):
            print(f'{path}:{n} {function}')
        return 0
    failures = check(args.root)
    if failures:
        print('\n'.join(failures))
        return 1
    print('Reference-test inventory passed.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
