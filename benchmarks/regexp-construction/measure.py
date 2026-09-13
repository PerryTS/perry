#!/usr/bin/env python3
"""Alternate the real-input probe against baseline Perry and Bun; retain samples."""
import argparse
import json
from pathlib import Path
import statistics
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--before', required=True)
parser.add_argument('--after', required=True)
parser.add_argument('--source', required=True)
parser.add_argument('--bun', required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--runs', type=int, default=7)
parser.add_argument('--case', action='append', help='MODE:SCALE; repeat to override the default cases')
args = parser.parse_args()
commands = {'before': [args.before], 'after': [args.after], 'bun': [args.bun, args.source]}
rows = []
cases = [(mode, int(scale)) for mode, scale in (case.split(':') for case in args.case)] if args.case else [
    ('all', 1), ('construct', 10), ('test-ascii', 100),
    ('test-emoji', 100), ('stripAnsi', 100), ('stripAnsi-match', 100)]
for mode, scale in cases:
    row = {'mode': mode, 'scale': scale, 'samples': {label: [] for label in commands}}
    expected = None
    for iteration in range(args.runs):
        order = list(commands)
        if iteration % 2:
            order.reverse()
        for label in order:
            result = subprocess.run(commands[label] + [mode, str(scale)], check=True,
                                    capture_output=True, text=True, timeout=180)
            times = {}
            other = []
            for line in result.stdout.splitlines():
                fields = line.split()
                if len(fields) == 3 and fields[2] == 'us/iter':
                    times[fields[0]] = float(fields[1])
                else:
                    other.append(line)
            if not times:
                raise RuntimeError(f'{mode} {label}: no timing output')
            if expected is None:
                expected = other
            if other != expected:
                raise RuntimeError(f'{mode} {label}: checksum mismatch {other} != {expected}')
            row['samples'][label].append(times)
    row['output'] = expected
    row['medians_us'] = {label: {name: statistics.median(s[name] for s in samples)
                                for name in samples[0]}
                         for label, samples in row['samples'].items()}
    rows.append(row)
    args.output.write_text(json.dumps(rows, indent=2) + '\n')
    print(json.dumps({k: v for k, v in row.items() if k != 'samples'}), flush=True)
