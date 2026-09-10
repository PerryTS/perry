#!/usr/bin/env python3
"""Validate all rotating trials and write their complete comparison table."""
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
    args = parser.parse_args()
    groups = defaultdict(lambda: defaultdict(list))
    for line in (args.results / 'timing.jsonl').read_text().splitlines():
        row = json.loads(line)
        if row.get('error') or row.get('exit_code') != 0:
            raise ValueError(f'Failed trial: {row}')
        groups[(row['fixture'], row['mode'])][row['engine']].append(row)
    host = json.loads((args.results / 'host.json').read_text())
    expected_engines = set(host.get('engines', ['perry', 'node', 'bun']))
    if not {'perry', 'node', 'bun'} <= expected_engines or expected_engines - {'perry', 'node', 'bun', 'baseline'}:
        raise ValueError('Invalid engine inventory')
    verify_counts = host.get('verification_counts_by_mode', {mode: [9] for mode in ['rotating', 'same', 'select']})
    expected = {(fixture['fixture'], mode) for fixture in host['fixtures']
                for mode in ['rotating', 'same', 'select']}
    if set(groups) != expected:
        raise ValueError('Incomplete modes or fixtures; this report requires all three modes')
    verified = [json.loads(line) for line in (args.results / 'verify.jsonl').read_text().splitlines()]
    expected_verification = {(fixture, mode, engine, count) for fixture, mode in expected
                             for engine in expected_engines for count in verify_counts[mode]}
    if (len(verified) != len(expected_verification)
            or {(row['fixture'], row['mode'], row['engine'], row['iterations']) for row in verified} != expected_verification
            or not all(row.get('correct') and row.get('verified_members') == list(range(8)) for row in verified)):
        raise ValueError('Missing or failed verification')
    summary = []
    for (fixture, mode), engines in groups.items():
        if set(engines) != expected_engines:
            raise ValueError(f'Missing engine: {fixture} {mode}')
        if len({r['iterations'] for rows in engines.values() for r in rows}) != 1:
            raise ValueError(f'Unequal engine work: {fixture} {mode}')
        for engine, rows in engines.items():
            if len(rows) != args.repeat or {r['rep'] for r in rows} != set(range(args.repeat)):
                raise ValueError(f'Wrong repetitions: {fixture} {mode} {engine}')
            if any(r['iterations'] <= 0 or not all(math.isfinite(r[k]) and r[k] >= 0
                   for k in ['user_us', 'system_us', 'wall_ms', 'rss_after', 'peak_rss']) for r in rows):
                raise ValueError(f'Invalid measurement: {fixture} {mode} {engine}')
            summary.append(dict(fixture=fixture, mode=mode, engine=engine,
                                cpu_us=statistics.median((r['user_us'] + r['system_us']) / r['iterations'] for r in rows),
                                wall_us=statistics.median(r['wall_ms'] * 1000 / r['iterations'] for r in rows),
                                rss_after_mib=statistics.median(r['rss_after'] / 1048576 for r in rows),
                                peak_rss_mib=statistics.median(r['peak_rss'] / 1048576 for r in rows)))
    (args.results / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    index = {(r['fixture'], r['mode'], r['engine']): r for r in summary}
    lines = ['Median process CPU in microseconds per call; RSS includes eight preloaded inputs.',
             'Rotating inputs retain equal byte size and shape and change one value.',
             'The same-source and selection-only controls keep those same eight inputs alive.',
             'Selection overhead is reported without subtraction. Host admission is a separate requirement.',
             '', '| Fixture | Mode | Perry µs | Node µs | Bun µs | Perry / best | Reference µs | Perry / reference |',
             '|---|---|---:|---:|---:|---:|---:|---:|']
    for fixture, mode in groups:
        m = {e: index[(fixture, mode, e)]['cpu_us'] for e in ['perry', 'node', 'bun']}
        ref = index.get((fixture, mode, 'baseline'))
        reference = f"{ref['cpu_us']:.6f} | {m['perry'] / ref['cpu_us']:.3f}" if ref else '— | —'
        lines.append(f"| {fixture} | {mode} | {m['perry']:.6f} | {m['node']:.6f} | {m['bun']:.6f} | "
                     f"{m['perry'] / min(m['node'], m['bun']):.3f} | {reference} |")
    lines += ['', 'RSS in MiB. Each engine retains the same eight-input pool.', '',
              '| Fixture | Mode | Peak: Perry / Node / Bun | After: Perry / Node / Bun | Reference peak / after |',
              '|---|---|---:|---:|---:|']
    for fixture, mode in groups:
        def values(metric):
            return ' / '.join(f"{index[(fixture, mode, e)][metric]:.3f}" for e in ['perry', 'node', 'bun'])
        ref = index.get((fixture, mode, 'baseline'))
        reference = f"{ref['peak_rss_mib']:.3f} / {ref['rss_after_mib']:.3f}" if ref else '—'
        lines.append(f"| {fixture} | {mode} | {values('peak_rss_mib')} | {values('rss_after_mib')} | {reference} |")
    (args.results / 'comparison.md').write_text('\n'.join(lines) + '\n')
    print(f'Validated {len(verified)} output comparisons and {len(summary) * args.repeat} timing trials')


if __name__ == '__main__':
    main()
