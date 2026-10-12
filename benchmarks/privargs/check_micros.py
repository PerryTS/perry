#!/usr/bin/env python3
"""Compile all lane/class micros and verify each checksum against Node."""
import argparse
import json
from pathlib import Path
from measure import MODES, CLASS_MICROS, compile_one, construct_sources, environment, run, normalized, measure_micro

p = argparse.ArgumentParser()
p.add_argument('root', type=Path)
p.add_argument('arm')
p.add_argument('--rank-buffer', action='store_true')
a = p.parse_args()
root = a.root.resolve()
construct_sources(root)
results = {}
for name in [*MODES, *CLASS_MICROS]:
    folder = 'micros' if name in MODES else 'class-micros'
    source = root / folder / (name + '.ts')
    assert compile_one(root, a.arm, name, source), name
    env = environment(root, a.arm)
    actual = run([str(root / 'bin' / a.arm / name), '200000'], env, root)
    expected = run(['node', '--allow-natives-syntax', str(source.with_suffix('.js')), '200000'], env, root)
    good = actual.returncode == expected.returncode == 0 and normalized(actual.stdout) == normalized(expected.stdout)
    results[name] = good
    print(name, good, flush=True)
    assert good, (name, actual.stdout, expected.stdout)
(root / ('micros-correctness-' + a.arm + '.json')).write_text(json.dumps(results, indent=2))
if a.rank_buffer:
    rows = {name: measure_micro(root, name, [a.arm, 'node'], root / 'micros' / (name + '.js'), [])
            for name in ['buffer', 'buffer_alias']}
    (root / 'rank-buffer.json').write_text(json.dumps(rows, indent=2))
