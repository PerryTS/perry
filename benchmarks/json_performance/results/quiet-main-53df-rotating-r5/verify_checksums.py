"""Check all recorded work counts against Node's verified per-call checksum.

Run from the repository root with the result directory as the sole argument.
The parsed-value checksum observes null/non-null; the separate eight-member
verification checks serialized contents. This is not a full conformance test.
"""
from pathlib import Path
import json
import math
import sys

root = Path(sys.argv[1])
verified = [json.loads(line) for line in (root / 'verify.jsonl').read_text().splitlines()]
units = {}
for row in verified:
    if row['engine'] != 'node':
        continue
    unit = row['checksum'] / (row['iterations'] + row['warmup'])
    key = row['fixture'], row['mode']
    assert unit.is_integer() and units.setdefault(key, unit) == unit, row

checked = {}
for phase in ['verify', 'calibration', 'timing']:
    path = root / (phase + '.jsonl')
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    for row in rows:
        assert not row.get('error') and row['exit_code'] == 0, (phase, row)
        assert all(math.isfinite(row[k]) for k in [
            'checksum', 'retained', 'rss_before', 'rss_after', 'peak_rss',
            'wall_ms', 'user_us', 'system_us',
        ]), (phase, row)
        expected = units[row['fixture'], row['mode']] * (row['iterations'] + row['warmup'])
        assert expected.is_integer() and abs(expected) <= 2**53, (phase, row)
        assert row['checksum'] == expected and row['retained'] == 0, (phase, row, expected)
    checked[phase] = len(rows)

(root / 'checksum-validation.json').write_text(json.dumps({
    'checked': checked,
    'all_finite': True,
    'all_checksums_match_node_per_call': True,
    'retained_counts_match': True,
}, indent=2) + '\n')
print('PASS rotating checksums and retained counts:', checked)
