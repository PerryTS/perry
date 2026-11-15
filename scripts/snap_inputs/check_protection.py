#!/usr/bin/env python3
"""Require the real protection transition and its failing base execution."""
import csv
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
arm = sys.argv[2]
assert arm in ('base', 'head'), arm
statuses = dict(line.split() for line in (root / 'results.tsv').read_text().splitlines())
for watch in (0, 1):
    expected = 139 if arm == 'base' and watch == 1 else 0
    assert int(statuses[str(watch)]) == expected, (arm, watch, statuses)
    path = root / f'report-{watch}.inputs.tsv'
    assert path.read_bytes().endswith(b'\n'), path
    events = list(csv.DictReader(path.open(), delimiter='\t'))
    assert [int(r['seq']) for r in events] == list(range(1, len(events) + 1)), path
    writes = [r for r in events if r['op'] == 'first_changed_tracked_word'
              and bytes.fromhex(r['value_hex']) != b'0']
    assert bool(writes) == bool(watch), (watch, writes)
    native = [json.loads(line) for line in (root / f'native-{watch}.jsonl').open()]
    assert native[-1]['kind'] == 'summary', native[-1]
    assert native[-1]['main_rc'] == expected, native[-1]
    none = [r for r in native if r['kind'] == 'syscall' and r.get('nr') == 10
            and r['args'][2] == 0 and r.get('ret') == 0]
    assert bool(none) == (arm == 'base' and watch == 1), (arm, watch, len(none))
    stdout = (root / f'stdout-{watch}').read_text()
    if expected == 0:
        assert stdout.splitlines() == ['before 3000', 'boundary complete 4496999'], stdout
    else:
        assert stdout.splitlines() == ['before 3000'], stdout
        assert 'snap census: foreign fault' in (root / f'stderr-{watch}').read_text()
    print(f'{arm} watch={watch}: exit={expected}, nonzero first writes={len(writes)}, '
          f'successful PROT_NONE={len(none)}')
