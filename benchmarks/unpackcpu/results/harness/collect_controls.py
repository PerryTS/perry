from collections import defaultdict
from pathlib import Path
from statistics import median
import json

ROOT = Path('/root/lanes/perry-unpackcpu')
groups = defaultdict(list)
kinds = ['programs-noise', 'upm-noise', 'micro-noise',
         'programs-thpoff', 'programs-thpoff-gc', 'upm-thpoff', 'upm-thpoff-gc',
         'micro-thpoff', 'micro-thpoff-gc']
for kind in kinds:
    for line in (ROOT / 'measure' / (kind + '.jsonl')).read_text().splitlines():
        row = json.loads(line)
        assert row['rc'] == 0 and row['parity']
        groups[(kind, row['name'], row['arm'])].append(row)
summary = []
for (kind, name, arm), rows in sorted(groups.items()):
    assert len(rows) == 5 and sorted(row['n'] for row in rows) == list(range(5))
    metrics = {}
    for key in ['instructions:u', 'wall', 'user', 'sys', 'rss_kb', 'fulls']:
        values = [row[key] for row in rows if row.get(key) is not None]
        if values:
            center = median(values)
            metrics[key] = dict(median=center, min=min(values), max=max(values),
                                mad=median(abs(value - center) for value in values))
    summary.append(dict(kind=kind, name=name, arm=arm, n=5, metrics=metrics))
(ROOT / 'evidence/controls.json').write_text(json.dumps(summary, indent=2) + '\n')
for kind in ['programs-noise', 'upm-noise', 'micro-noise']:
    for name in sorted({row['name'] for row in summary if row['kind'] == kind}):
        base, repeat = [next(row for row in summary if row['kind'] == kind and
                             row['name'] == name and row['arm'] == arm)
                        for arm in ['base', 'repeat']]
        b, r = [row['metrics']['instructions:u']['median'] for row in [base, repeat]]
        print(kind, name, f'{(r / b - 1) * 100:+.5f}%')
