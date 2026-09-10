#!/usr/bin/env python3
"""Validate and summarize a complete original JSON benchmark run, including A/B.

Requires every declared operation, retained-output group, engine and repetition.
All successful samples contribute to medians; this does not declare significance.
"""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import statistics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('results', type=Path)
    parser.add_argument('--repeat', type=int, default=5)
    parser.add_argument('--engines', default='perry,node,bun')
    args = parser.parse_args()
    engines = set(args.engines.split(','))
    root = Path(__file__).resolve().parent
    fixtures = json.loads((root / 'results/fixtures.json').read_text())
    operations = {(f['name'], op) for f in fixtures for op in f['operations']}
    retained = {'tiny_object': 200000, 'small_record': 100000,
                'records_array_1m': 16, 'records_object_1m': 16,
                'records_array_8m': 4, 'records_object_8m': 4,
                'long_string_1m': 32, 'unicode_1m': 32, 'wide_1m': 16}
    window = json.loads((args.results / 'window.json').read_text())
    if window.get('quiet_gate_passed') is not True:
        raise ValueError('Quiet-host admission did not pass')

    def read(phase):
        rows = [json.loads(line) for line in
                (args.results / f'{phase}.jsonl').read_text().splitlines()]
        for row in rows:
            if row.get('error') or row.get('exit_code') != 0:
                raise ValueError(f'Failed {phase} trial: {row}')
        return rows

    verified = read('verify')
    expected_checks = {(f, op, e) for f, op in operations for e in engines}
    actual_checks = [(r['fixture'], r['operation'], r['engine']) for r in verified]
    if set(actual_checks) != expected_checks or len(actual_checks) != len(expected_checks):
        raise ValueError('Missing or duplicate correctness cells')
    if not all(r.get('correct') is True for r in verified):
        raise ValueError('Correctness comparison failed')

    summary = []
    for phase in ['timing', 'memory']:
        groups = defaultdict(list)
        for row in read(phase):
            groups[(row['fixture'], row['operation'], row['iterations'], row['engine'])].append(row)
        if phase == 'memory':
            expected = {(f, op, n, e) for f, count in retained.items()
                        for op in ['retain-parse', 'retain-stringify']
                        for n in [1, count] for e in engines}
            if set(groups) != expected:
                raise ValueError('Missing or unexpected retained-output cells')
        else:
            cells = {(f, op, e) for f, op, _, e in groups}
            if cells != expected_checks or len(groups) != len(expected_checks):
                raise ValueError('Missing or duplicate timing cells')
            for fixture, operation in operations:
                counts = {n for f, op, n, _ in groups if (f, op) == (fixture, operation)}
                if len(counts) != 1:
                    raise ValueError(f'Unequal engine work: {fixture}/{operation}')
        for (fixture, operation, count, engine), rows in sorted(groups.items()):
            if len(rows) != args.repeat or {r['rep'] for r in rows} != set(range(args.repeat)):
                raise ValueError(f'Missing/duplicate repetitions: {phase}/{fixture}/{operation}/{engine}')
            if count <= 0:
                raise ValueError('Nonpositive iteration count')
            metrics = {
                'cpu_us': [(r['user_us'] + r['system_us']) / count for r in rows],
                'wall_us': [r['wall_ms'] * 1000 / count for r in rows],
                'peak_rss_mib': [r['peak_rss'] / 1048576 for r in rows],
                'rss_after_mib': [r['rss_after'] / 1048576 for r in rows],
            }
            if not all(math.isfinite(v) and v >= 0 for values in metrics.values() for v in values):
                raise ValueError('Invalid measurement')
            summary.append(dict(phase=phase, fixture=fixture, operation=operation,
                                iterations=count, engine=engine, repetitions=len(rows),
                                **{k: statistics.median(v) for k, v in metrics.items()},
                                cpu_samples_us=metrics['cpu_us'],
                                peak_rss_samples_mib=metrics['peak_rss_mib']))
    (args.results / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    order = [engine for engine in ['perry', 'node', 'bun', 'baseline'] if engine in engines]
    cells = defaultdict(dict)
    for row in summary:
        cells[(row['phase'], row['fixture'], row['operation'], row['iterations'])][row['engine']] = row
    lines = ['Complete original comparison: medians from fresh processes with equal work.',
             'CPU is process CPU per call; RSS is process memory, including runtime and inputs.',
             'Repeated-source parse can use Perry caches. Changing-source results are reported separately.',
             'Median differences alone do not establish statistical significance.', '']
    for metric, title in [('cpu_us', 'CPU, microseconds per call'),
                          ('peak_rss_mib', 'Peak RSS, MiB'),
                          ('rss_after_mib', 'RSS after the measured loop, MiB')]:
        lines += [f'## {title}', '',
                  '| Fixture | Operation | Calls / retained | ' + ' | '.join(order) + ' |',
                  '|---|---|---:|' + '---:|' * len(order)]
        for (phase, fixture, operation, count), rows in sorted(cells.items()):
            if metric == 'cpu_us' and phase != 'timing':
                continue
            values = ' | '.join(f'{rows[engine][metric]:.6f}' for engine in order)
            lines.append(f'| {fixture} | {operation} | {count} | {values} |')
        lines.append('')
    (args.results / 'comparison.md').write_text('\n'.join(lines).rstrip() + '\n')
    print(f'Validated {len(verified)} correctness checks and {len(summary)} measurement groups')


if __name__ == '__main__':
    main()
