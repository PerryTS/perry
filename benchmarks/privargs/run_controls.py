#!/usr/bin/env python3
"""Both controls must break Node parity while the untouched binary passes."""
import json
import subprocess
import sys
from pathlib import Path
from measure import environment

root = Path(sys.argv[1]).resolve()
results = {}
for mode, stem in [('arguments', 'test_gap_privargs_lazy_arguments'),
                   ('snapshot', 'test_gap_10509_arguments_reads'),
                   ('home', 'test_gap_privargs_method_home')]:
    source = root / 'focused' / ('head-' + stem)
    dest = root / 'focused' / (mode + '-negative-control')
    patched = subprocess.run([sys.executable, str(Path(__file__).with_name('negative.py')),
                              str(source), str(dest), mode], capture_output=True, check=True)
    expected = (root / 'focused' / (stem + '.node.out')).read_bytes()
    env = environment(root, 'head')
    positive = subprocess.run([str(source)], capture_output=True, env=env)
    negative = subprocess.run([str(dest)], capture_output=True, env=env)
    (root / (mode + '-control.out')).write_bytes(negative.stdout)
    (root / (mode + '-control.err')).write_bytes(negative.stderr)
    row = {'positive_matches_node': positive.returncode == 0 and positive.stdout == expected,
           'negative_matches_node': negative.returncode == 0 and negative.stdout == expected,
           'changed': patched.stdout.decode().strip(), 'negative_exit': negative.returncode}
    assert row['positive_matches_node'] and not row['negative_matches_node'], row
    results[mode] = row
    print(mode, row, flush=True)
(root / 'controls.json').write_text(json.dumps(results, indent=2))
