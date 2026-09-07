#!/usr/bin/env python3
"""Summarize matched lifetime probes without mixing them into the main matrix."""
from pathlib import Path
import json
import statistics
import sys

root = Path(sys.argv[1])
rows = [json.loads(line) for line in (root / 'raw.jsonl').read_text().splitlines()]
keys = sorted({(r['fixture'], r['operation'], r['mode']) for r in rows})
assert len(keys) == 42 and len(rows) == 252
metrics = ['parseMs', 'stringifyMs', 'parseMaxMs', 'stringifyMaxMs', 'loopCpuUs',
           'drainCpuUs', 'totalCpuUs', 'loopMs', 'drainMs', 'peak_rss', 'rssLoop', 'rssDrained']
summaries = []
for fixture, operation, mode in keys:
    selected = [r for r in rows if (r['fixture'], r['operation'], r['mode']) == (fixture, operation, mode)]
    assert len({r['iterations'] for r in selected}) == 1
    assert len({r['checksum'] for r in selected}) == 1
    if mode in ['latest', 'retain']:
        assert all(r['output_verified'] for r in selected)
    result = dict(fixture=fixture, operation=operation, mode=mode, iterations=selected[0]['iterations'])
    for metric in metrics:
        values = {engine: [r[metric] for r in selected if r['engine'] == engine]
                  for engine in ['parent', 'candidate']}
        assert all(len(v) == 3 for v in values.values())
        parent, candidate = (statistics.median(values[e]) for e in ['parent', 'candidate'])
        ranges = {engine: [min(value), max(value)] for engine, value in values.items()}
        result[metric] = dict(parent=parent, candidate=candidate,
                              delta_percent=(candidate / parent - 1) * 100 if parent else None,
                              ranges=ranges,
                              ranges_separated=ranges['parent'][1] < ranges['candidate'][0]
                              or ranges['candidate'][1] < ranges['parent'][0])
    summaries.append(result)
(root / 'summary.json').write_text(json.dumps(summaries, indent=2) + '\n')
lines = ['# Supplemental lifetime comparisons', '',
         'Each row has three matched fresh-process pairs. Total CPU includes the loop and a final explicit collection. '
         'API timing includes per-call clocks; these rows are separate from the fixed 38-row baseline. '
         'Peak RSS is whole-process, including setup and post-timing verification. '
         '“Separated” means the observed total-CPU ranges do not overlap; it is not a statistical confidence interval.', '',
         '| Fixture | Operation | Lifetime | Calls | API µs parent → candidate | Total CPU change | Separated | Peak RSS change MiB |',
         '|---|---|---|---:|---:|---:|---|---:|']
for row in summaries:
    n = row['iterations']
    api = {engine: (row['parseMs'][engine] + row['stringifyMs'][engine]) * 1000 / n
           for engine in ['parent', 'candidate']}
    peak_delta = (row['peak_rss']['candidate'] - row['peak_rss']['parent']) / 1048576
    lines.append(f"| {row['fixture']} | {row['operation']} | {row['mode']} | {n} | "
                 f"{api['parent']:.3f} → {api['candidate']:.3f} | {row['totalCpuUs']['delta_percent']:+.2f}% | "
                 f"{row['totalCpuUs']['ranges_separated']} | {peak_delta:+.3f} |")
(root / 'table.md').write_text('\n'.join(lines) + '\n')
print('Validated and summarized 42 lifetime cases / 252 paired trials')
