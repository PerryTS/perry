#!/usr/bin/env python3
"""Two-count, n=5 instruction/RSS proof of each prototype-birth change."""
import argparse
import ctypes
import json
from pathlib import Path
import statistics
import re
import subprocess

import perf_schemadec_lane as common


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--arms', default='main,node,head')
    parser.add_argument('--label', default='final')
    args = parser.parse_args()
    root = args.hostdir.resolve()
    out = root / 'micros' / args.label
    out.mkdir(parents=True, exist_ok=True)
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE')
    source = root / 'src/benchmarks/schemadec/prototype_birth.ts'
    results = {}
    arms = args.arms.split(',')
    commands = {}
    samples = {}
    expected = {}
    for mode in (0, 1):
        for n in (20000, 200000):
            expected[mode, n] = common.common.execute(
                ['node', str(source), str(n), str(mode)], source.parent,
                common.environment(root, 'head'), out / f'oracle.mode{mode}.N{n}').stdout
    for arm in arms:
        env = common.environment(root, arm)
        binary = out / arm
        if arm != 'node':
            common.common.execute([str(Path(env['CARGO_TARGET_DIR']) / 'release/perry'),
                                   'compile', str(source), '-o', str(binary)],
                                  source.parent, env, out / f'{arm}.compile')
            text_size = int(re.search(r'^\.text\s+(\d+)', subprocess.check_output(
                ['size', '-A', str(binary)], text=True), re.M)[1])
        else:
            text_size = None
        commands[arm] = ['node', str(source)] if arm == 'node' else [str(binary)]
        samples[arm] = {mode: {n: [] for n in (20000, 200000)} for mode in (0, 1)}
        results[arm] = {'modes': {}, 'text': text_size}
    for mode in (0, 1):
        for trial in range(5):
            for n in (20000, 200000):
                for arm in (arms if trial % 2 == 0 else arms[::-1]):
                    env = common.environment(root, arm)
                    prefix = out / f'{arm}.mode{mode}.trial{trial}.N{n}'
                    result = common.common.execute([
                        'setarch', '-R', 'perf', 'stat', '-x', ';', '-e', 'instructions:u',
                        '-o', str(prefix) + '.stat', '/usr/bin/time', '-f', '%M',
                        '-o', str(prefix) + '.rss', *commands[arm], str(n), str(mode)],
                        source.parent, env, prefix)
                    assert result.stdout == expected[mode, n], (arm, mode, n)
                    instructions = next(int(line.split(';')[0]) for line in
                                        Path(str(prefix) + '.stat').read_text().splitlines()
                                        if ';instructions:u;' in line)
                    samples[arm][mode][n].append({'instructions': instructions,
                                                 'rss_kb': int(Path(str(prefix) + '.rss').read_text())})
    for arm in arms:
        for mode, trials in samples[arm].items():
            results[arm]['modes'][mode] = {'trials': trials, 'instructions_per_op': (
                statistics.median(row['instructions'] for row in trials[200000]) -
                statistics.median(row['instructions'] for row in trials[20000])) / 180000,
                'rss_kb': statistics.median(row['rss_kb'] for row in trials[200000])}
        (out / 'results.json').write_text(json.dumps(results, indent=2))
        print(args.label, arm, {mode: row['instructions_per_op'] for mode, row in results[arm]['modes'].items()},
              'text', results[arm]['text'], flush=True)


if __name__ == '__main__':
    main()
