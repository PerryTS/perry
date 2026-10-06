#!/usr/bin/env python3
"""Ratchet byte-layout coupling until the remaining owners land B4c/B5.

Only buffer accessors and ABI constants may introduce a layout dependency.
Existing codegen, zlib, fetch and node_stream sites remain explicit debt;
removing a site requires removing its baseline entry in the same commit.
"""
import argparse
import collections
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
BASE = ROOT / 'scripts/buffer_layout_baseline.json'
PATTERNS = [
    re.compile(r'size_of\s*::\s*<\s*(?:[\w:]+::)?(?:BufferHeader|TypedArrayHeader)\s*>'),
    re.compile(r'\b(?:buf(?:fer)?(?:_data|_ptr)?|ta|view|result)\s*(?:as\s+\*\s*(?:const|mut)\s+u8\s*)?\)?\s*\.add\(\s*(?:8|16)\s*\)'),
    re.compile(r'\(\*(?:buf(?:fer)?(?:_ptr)?|ta|view|result|backing)\)\.(?:length|capacity)\s*=(?!=)'),
    re.compile(r'(?<!fn )\b(?:buffer_data(?:_mut)?|typed_array_bytes(?:_mut)?|js_value_buffer_or_typedarray_data|js_native_buffer_data_ptr)\s*\('),
    re.compile(r'\btypedarray::data_ptr(?:_mut)?\s*\('),
]
# These are the layout/accessor implementations, rather than byte consumers.
ACCESSORS = {
    'crates/perry-runtime/src/buffer/header.rs',
    'crates/perry-runtime/src/buffer/bytes.rs',
    'crates/perry-runtime/src/typedarray/mod.rs',
}
EMITTERS = {
    'function.rs', 'buffer_views.rs', 'i32_fast_path.rs', 'inline_dyn_typed_array.rs',
    'index_set_typed_array.rs', 'ta_element_read.rs', 'typed_array_rmw.rs',
    'u8_buffer_read.rs', 'arrays.rs', 'stable_packed_typed_array.rs',
}
EMITTED = re.compile(r'\b(?:add|gep)\([^\n]*"(?:8|16|10)"')

def inventory(root=ROOT):
    out = collections.Counter()
    for p in sorted((root / 'crates').rglob('*.rs')):
        relative = p.relative_to(root).as_posix()
        if relative.startswith('crates/perry-abi/') or '/buffer/store' in relative or relative in ACCESSORS:
            continue
        for line in p.read_text().splitlines():
            code = line.strip()
            if code.startswith('//'):
                continue
            emitted = '/perry-codegen/' in relative and p.name in EMITTERS and EMITTED.search(code)
            if emitted or any(pattern.search(code) for pattern in PATTERNS):
                out[relative + '|' + re.sub(r'\s+', ' ', code)] += 1
    return out

def self_test():
    # Exercise the executable gate, including its exit status, not only regexes.
    with tempfile.TemporaryDirectory(prefix='.buffer-gate-', dir=ROOT) as directory:
        root = Path(directory)
        crate = root / 'crates/perry-ext-fixture/src'
        crate.mkdir(parents=True)
        baseline = root / 'baseline.json'
        baseline.write_text('{}\n')
        emitted = root / 'crates/perry-codegen/src/function.rs'
        emitted.parent.mkdir(parents=True)
        fixtures = [(crate / 'lib.rs', code) for code in [
            'let dst = (buf as *mut u8).add(8);',
            '(*buffer).length = 8;',
            'std::mem::size_of::<crate::buffer::BufferHeader>()',
            'let ptr = crate::buffer::buffer_data(buffer);',
            'let data = typed_array_bytes(ta);',
            'let data = js_value_buffer_or_typedarray_data(value, &mut len);',
            'let data = crate::typedarray::data_ptr_mut(ta);',
        ]] + [(emitted, code) for code in [
            'let data = blk.add(I64, &raw, "8");',
            'let data = blk.gep(I8, &header, &[(I32, "16")]);',
        ]]
        last = None
        for index, (fixture, planted) in enumerate(fixtures):
            if last is not None: last.unlink()
            fixture.write_text(planted + '\n')
            child = subprocess.run([sys.executable, __file__, '--root', str(root),
                                    '--baseline', str(baseline)], capture_output=True, text=True)
            assert child.returncode == 1 and child.stdout.count('new:') == 1, (planted, child.stdout, child.stderr)
            print(f'buffer-layout sabotage {index + 1}: RED')
            last = fixture
        last.unlink()
        (crate / 'lib.rs').write_text('// buffer_data(buffer);\nlet n = array.length;\n')
        assert not inventory(root), 'unrelated arrays and comments must stay outside the gate'

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--update', action='store_true', help='review and commit every baseline change')
    ap.add_argument('--self-test', action='store_true')
    ap.add_argument('--root', type=Path, default=ROOT)
    ap.add_argument('--baseline', type=Path, default=BASE)
    args = ap.parse_args()
    if args.self_test:
        self_test()
        return
    current = inventory(args.root)
    if args.update:
        args.baseline.write_text(json.dumps(dict(sorted(current.items())), indent=2) + '\n')
        print(f'buffer-layout baseline: {sum(current.values())} sites')
        return
    baseline = collections.Counter(json.loads(args.baseline.read_text()))
    added, stale = current - baseline, baseline - current
    if added or stale:
        for label, entries in [('new', added), ('stale (ratchet down)', stale)]:
            for site, n in entries.items():
                print(f'{label}: {n} x {site}')
        raise SystemExit(1)
    if args.root == ROOT:
        wrapper = (ROOT / 'crates/perry-ffi/src/buffer.rs').read_text()
        assert 'bytes::from_slice' in wrapper
        assert '.add(' not in wrapper and 'copy_nonoverlapping' not in wrapper
    print(f'buffer-layout ratchet: {sum(current.values())} existing sites, 0 new')

if __name__ == '__main__':
    main()
