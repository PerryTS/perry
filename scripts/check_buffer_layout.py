#!/usr/bin/env python3
"""Ratchet explicit byte-layout coupling, independent of census line numbers.

A textual gate cannot infer Rust receiver types; the recognisers deliberately
cover named byte headers and obvious byte receiver arithmetic. B5 privacy is
what makes the full invariant a compile-time proof.
"""
import argparse
import collections
import json
from pathlib import Path
import re
ROOT = Path(__file__).resolve().parent.parent
BASE = ROOT / 'scripts/buffer_layout_baseline.json'
PATTERNS = [
    re.compile(r'size_of\s*::\s*<\s*(?:[\w:]+::)?(?:BufferHeader|TypedArrayHeader)\s*>'),
    re.compile(r'\b(?:buf(?:fer)?(?:_data|_ptr)?|ta|view|result)\s*(?:as\s+\*\s*(?:const|mut)\s+u8\s*)?\)?\s*\.add\(\s*(?:8|16)\s*\)'),
    re.compile(r'\.(?:add|gep)\([^;]*"(?:8|16|10)"'),
    re.compile(r'\(\*(?:buf(?:fer)?(?:_ptr)?|ta|view|result|backing)\)\.(?:length|capacity)\s*=(?!=)'),
]
def inventory(root=ROOT):
    out = collections.Counter()
    for p in (root / 'crates').rglob('*.rs'):
        relative = p.relative_to(root).as_posix()
        if relative.startswith('crates/perry-abi/') or '/buffer/store' in relative:
            continue
        for line in p.read_text().splitlines():
            code = line.strip()
            if code.startswith('//'):
                continue
            if any(pattern.search(code) for pattern in PATTERNS):
                out[relative + '|' + re.sub(r'\s+', ' ', code)] += 1
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--update', action='store_true', help='review and commit every baseline change')
    ap.add_argument('--self-test', action='store_true')
    args = ap.parse_args()
    if args.self_test:
        for planted in ['let dst = (buf as *mut u8).add(8);', '(*buffer).length = 8;',
                        'std::mem::size_of::<crate::buffer::BufferHeader>()',
                        'let data = blk.add(I64, &raw, "8");',
                        'let data = blk.gep(I8, &header, &[(I32, "16")]);']:
            assert any(p.search(planted) for p in PATTERNS), planted
        print('buffer-layout planted writers: RED')
        return
    current = inventory()
    if args.update:
        BASE.write_text(json.dumps(dict(sorted(current.items())), indent=2) + '\n')
        print(f'buffer-layout baseline: {sum(current.values())} sites')
        return
    baseline = collections.Counter(json.loads(BASE.read_text()))
    added = current - baseline
    stale = baseline - current
    if added or stale:
        for label, entries in [('new', added), ('stale (ratchet down)', stale)]:
            for site, n in entries.items():
                print(f'{label}: {n} x {site}')
        raise SystemExit(1)
    # Wrapper privacy is stronger than a text pattern: sizeof this type no
    # longer tells a binding where bytes are. Also enforce the single route.
    wrapper = (ROOT / 'crates/perry-ffi/src/buffer.rs').read_text()
    assert 'bytes::from_slice' in wrapper
    assert '.add(' not in wrapper and 'copy_nonoverlapping' not in wrapper
    print(f'buffer-layout ratchet: {sum(current.values())} existing sites, 0 new')
if __name__ == '__main__':
    main()
