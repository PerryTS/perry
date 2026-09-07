#!/usr/bin/env python3
"""Recreate the layout-only workers from the pinned private artifact directory."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys

art, out = map(Path, sys.argv[1:])
out.mkdir(parents=True, exist_ok=True)
expected = json.loads((Path(__file__).parent / 'provenance.json').read_text())
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
assert sha(art / 'worker.o') == expected['object_sha256']
symbols = subprocess.check_output(['nm', '-n', str(art / 'defer2-worker')], text=True)
order = out / 'checkpoint.order'
order.write_text('\n'.join(line.split()[2] for line in symbols.splitlines()
                           if len(line.split()) == 3 and line.split()[1] in ['t', 'T']) + '\n')
assert sha(order) == expected['order_sha256']
for arm in ['defer2', 'defer4']:
    runtime = art / (arm + '-runtime/libperry_runtime.a')
    assert sha(runtime) == expected[arm]['runtime_sha256']
    with (out / (arm + '-link.log')).open('w') as log:
        subprocess.run(['cc', str(art / 'worker.o'), str(runtime), '-lc',
                        '-Wl,-dead_strip', '-Wl,-no_exported_symbols',
                        '-Wl,-order_file,' + str(order), '-Wl,-order_file_statistics',
                        '-o', str(out / (arm + '-ordered-worker'))],
                       check=True, stdout=log, stderr=subprocess.STDOUT)
