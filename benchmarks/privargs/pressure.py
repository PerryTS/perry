#!/usr/bin/env python3
"""Require the retained snapshot witness to survive actual copying GC."""
import json
import sys
from pathlib import Path
from measure import environment, run

root = Path(sys.argv[1]).resolve()
arm = sys.argv[2]
stem = 'test_gap_privargs_arguments_frame_gc'
binary = root / 'focused' / (arm + '-' + stem)
actual = run([str(binary)], dict(environment(root, arm), PERRY_GC_TRACE='1'), root)
expected = (root / 'focused' / (stem + '.node.out')).read_bytes()
assert actual.returncode == 0 and actual.stdout == expected
(root / ('pressure-' + arm + '.gc')).write_bytes(actual.stderr)
cycles = []
for line in actual.stderr.splitlines():
    try:
        event = json.loads(line)
    except ValueError:
        continue
    if event.get('event') == 'gc_cycle':
        cycles.append(event)
copied = sum(row.get('copying_nursery', {}).get('copied_objects', 0) for row in cycles)
assert copied > 0, 'no copying collection exercised the retained snapshots'
result = {'matches_node': True, 'copied_objects': copied,
          'full_collections': sum(row.get('collection_kind') == 'full' for row in cycles),
          'minor_collections': sum(row.get('collection_kind') == 'minor' for row in cycles)}
(root / ('pressure-' + arm + '.json')).write_text(json.dumps(result, indent=2))
print(result)
