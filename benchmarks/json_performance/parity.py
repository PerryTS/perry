#!/usr/bin/env python3
"""Report every CPU/RSS target; do not hide losing rows behind an average.

Ratios compare Perry with the better Node/Bun median and the reference Perry
median. This is a performance inventory, not a correctness or significance test.
Overlapping/noisy timing samples need longer repeat measurements before claiming
parity or a regression. Correctness and GC checks remain separate requirements.
"""
import argparse
import json
from pathlib import Path


def key(row):
    return row['phase'], row['fixture'], row['operation'], row['iterations']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('summary', type=Path)
    parser.add_argument('--reference', type=Path, help='previous full summary; default: baseline arm')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    rows = json.loads(args.summary.read_text())
    reference = json.loads(args.reference.read_text()) if args.reference else rows
    reference_engine = 'perry' if args.reference else 'baseline'
    refs = {key(row): row for row in reference if row['engine'] == reference_engine}
    groups = {}
    for row in rows:
        groups.setdefault(key(row), {})[row['engine']] = row
    result = []
    for group, engines in groups.items():
        if not all(engine in engines for engine in ['perry', 'node', 'bun']):
            raise ValueError(f'Incomplete engines: {group}')
        perry = engines['perry']
        metrics = (['cpu_us', 'peak_rss_mib'] if perry['phase'] == 'timing'
                   else ['rss_after_mib', 'peak_rss_mib'])
        for metric in metrics:
            best = min(['node', 'bun'], key=lambda engine: engines[engine][metric])
            target = engines[best][metric]
            ref = refs.get(group)
            result.append({
                'phase': group[0], 'fixture': group[1], 'operation': group[2],
                'iterations': group[3], 'metric': metric,
                'perry': perry[metric], 'target_engine': best, 'target': target,
                'target_ratio': perry[metric] / target,
                'at_or_below_target_median': perry[metric] <= target,
                'reference': ref[metric] if ref else None,
                'reference_ratio': perry[metric] / ref[metric] if ref else None,
            })
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'parity.json').write_text(json.dumps(result, indent=2) + '\n')
    lines = [
        'Every measured CPU/RSS target, compared with the better Node/Bun median.',
        'A ratio above 1 means Perry is behind. Median comparisons alone do not establish statistical significance.',
        'Correctness and GC validation are separate; this inventory does not declare the goal complete.',
        '', '| Fixture / operation | Metric | Calls / retained | Perry | Best engine | Target | Ratio | Reference ratio |',
        '|---|---|---:|---:|---|---:|---:|---:|',
    ]
    for row in result:
        ref = f"{row['reference_ratio']:.3f}×" if row['reference_ratio'] is not None else 'missing'
        lines.append(f"| {row['fixture']} / {row['operation']} | {row['metric']} | {row['iterations']} | "
                     f"{row['perry']:.3f} | {row['target_engine']} | {row['target']:.3f} | "
                     f"{row['target_ratio']:.3f}× | {ref} |")
    (args.output / 'parity.md').write_text('\n'.join(lines) + '\n')
    for metric in sorted({row['metric'] for row in result}):
        selected = [row for row in result if row['metric'] == metric]
        print(metric, 'at/below best median:', sum(row['at_or_below_target_median'] for row in selected),
              '/', len(selected))
    print('Reference rows missing:', sum(row['reference'] is None for row in result))


if __name__ == '__main__':
    main()
