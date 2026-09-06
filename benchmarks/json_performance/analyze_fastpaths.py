#!/usr/bin/env python3
"""Validate and summarize the four-arm runtime A/B without dropping trials."""
import collections
import argparse
import csv
import json
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--results-dir', type=Path, default=ROOT / 'results/fastpaths')
args = parser.parse_args()
OUT = args.results_dir
ENGINES = ['baseline', 'perry', 'node', 'bun']


def read(phase):
    return [json.loads(s) for s in (OUT / (phase + '.jsonl')).read_text().splitlines()]


verified = read('verify')
assert len(verified) == 152 and all(r['correct'] for r in verified)
summary = []
for phase in ['timing', 'memory']:
    groups = collections.defaultdict(list)
    for r in read(phase):
        assert 'error' not in r, r
        groups[r['fixture'], r['operation'], r['engine'], r['iterations']].append(r)
    for (fixture, operation, engine, n), rows in groups.items():
        assert len(rows) == 3, (fixture, operation, engine, len(rows))
        cpu = [r['cpu_ms'] * 1000 / n for r in rows]
        summary.append(dict(
            phase=phase, fixture=fixture, operation=operation, engine=engine, iterations=n,
            cpu_us=statistics.median(cpu), cpu_min_us=min(cpu), cpu_max_us=max(cpu),
            wall_us=statistics.median(r['wall_ms'] * 1000 / n for r in rows),
            rss_before_mib=statistics.median(r['rss_before'] for r in rows) / 1048576,
            rss_after_mib=statistics.median(r['rss_after'] for r in rows) / 1048576,
            peak_rss_mib=statistics.median(r['peak_rss'] for r in rows) / 1048576,
            peak_footprint_mib=statistics.median(r['peak_footprint'] for r in rows) / 1048576,
        ))
with (OUT / 'summary.csv').open('w') as f:
    writer = csv.DictWriter(f, fieldnames=list(summary[0]),lineterminator="\n")
    writer.writeheader()
    writer.writerows(summary)
(OUT / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
lookup = {(r['phase'], r['fixture'], r['operation'], r['engine'], r['iterations']): r for r in summary}
lines = [
    'Medians of three fresh processes; CPU includes user + system time.', '',
    'Baseline and patched Perry use freshly rebuilt runtime/stdlib archives and the same compiler.',
    'Node 26.5.1; Bun 1.3.14. Default GC behavior. Array-root parse may return a lazy Perry value;',
    'stringify starts with a fully materialized value. RSS is whole-process memory.', '',
    '| Fixture / operation | Calls | CPU µs: base / patched / Node / Bun | Perry speedup | Peak RSS MiB: base / patched / Node / Bun |',
    '|---|---:|---:|---:|---:|',
]
gains = []
for r in summary:
    if r['phase'] != 'timing' or r['engine'] != 'baseline':
        continue
    rows = [lookup['timing', r['fixture'], r['operation'], e, r['iterations']] for e in ENGINES]
    speedup = rows[0]['cpu_us'] / rows[1]['cpu_us']
    cpu = ' / '.join(f"{v['cpu_us']:,.3f}" for v in rows)
    rss = ' / '.join(f"{v['peak_rss_mib']:.2f}" for v in rows)
    lines.append(f"| {r['fixture']} / {r['operation']} | {r['iterations']:,} | {cpu} | {speedup:.2f}× | {rss} |")
    gains.append(dict(fixture=r['fixture'], operation=r['operation'], speedup=speedup,
                      baseline_cpu_us=rows[0]['cpu_us'], patched_cpu_us=rows[1]['cpu_us'],
                      baseline_peak_rss_mib=rows[0]['peak_rss_mib'], patched_peak_rss_mib=rows[1]['peak_rss_mib']))
lines += ['', 'Retained outputs (current RSS after the loop; all results remain live):', '',
          '| Fixture / operation | Retained results | RSS MiB: base / patched / Node / Bun |', '|---|---:|---:|']
for r in summary:
    if r['phase'] == 'memory' and r['engine'] == 'baseline':
        rows = [lookup['memory', r['fixture'], r['operation'], e, r['iterations']] for e in ENGINES]
        rss = ' / '.join(f"{v['rss_after_mib']:.2f}" for v in rows)
        lines.append(f"| {r['fixture']} / {r['operation']} | {r['iterations']:,} | {rss} |")
(OUT / 'tables.md').write_text('\n'.join(lines) + '\n')
(OUT / 'gains.json').write_text(json.dumps(gains, indent=2) + '\n')
for g in gains:
    print(g['fixture'], g['operation'], f"{g['speedup']:.3f}x", f"{g['baseline_cpu_us']:.3f} -> {g['patched_cpu_us']:.3f} us")
print('Validated', len(verified), 'outputs;', len(summary), 'measurement groups.')
