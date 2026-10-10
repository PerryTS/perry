#!/usr/bin/env python3
"""Interleaved instruction/RSS measurements of the two generic round-two cases."""
import argparse
import json
from pathlib import Path
import statistics
from perf_missread_lane import environment, run, NODE

p = argparse.ArgumentParser()
p.add_argument('--hostdir', type=Path, required=True)
root = p.parse_args().hostdir.resolve()
out = root / 'cases' / 'trials2'
out.mkdir(parents=True, exist_ok=True)
results = {}
for name in ['test_gap_missread_constructor_transitions', 'test_gap_missread_native_alias_holders']:
    case = f'test-files/{name}.ts'
    expected = (root / 'cases/main' / f'{name}.node.out').read_bytes()
    rows = {arm: [] for arm in ['main', 'head', 'node']}
    for trial in range(5):
        order = ['main', 'head', 'node'] if trial % 2 == 0 else ['node', 'head', 'main']
        for arm in order:
            label = f'{name}.{trial}.{arm}'
            counters, rss = out / f'{label}.stat', out / f'{label}.rss'
            command = [*NODE, case] if arm == 'node' else [str(root / 'cases' / arm / name)]
            _, _, env = environment(root, 'main' if arm == 'node' else arm)
            actual = run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', str(counters),
                          '/usr/bin/time', '-f', '%M', '-o', str(rss), *command],
                         root / 'src', env, out / label)
            if actual != expected:
                raise RuntimeError(f'{label}: output differs from Node')
            instructions = next(int(line.split(';')[0]) for line in counters.read_text().splitlines()
                                if ';instructions:u;' in line)
            rows[arm].append({'instructions': instructions, 'rss_kb': int(rss.read_text().strip())})
    results[name] = {'trials': rows, 'medians': {
        arm: {key: statistics.median(row[key] for row in values) for key in values[0]}
        for arm, values in rows.items()}}
    print(name, results[name]['medians'], flush=True)
(root / 'cases' / 'metrics2.json').write_text(json.dumps(results, indent=2))
