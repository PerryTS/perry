#!/usr/bin/env python3
"""Alternate the real-input probe against baseline Perry and Bun; retain samples."""
import argparse
import json
from pathlib import Path
import shutil
import statistics
import subprocess


def positive_integer(value):
    try:
        number = int(value)
    except ValueError:
        raise argparse.ArgumentTypeError('expected a positive integer') from None
    if number < 1:
        raise argparse.ArgumentTypeError('expected a positive integer')
    return number


def probe_case(value):
    mode, separator, scale = value.partition(':')
    if not mode or not separator:
        raise argparse.ArgumentTypeError('expected MODE:SCALE')
    return mode, positive_integer(scale)


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--before', required=True)
parser.add_argument('--after', required=True)
parser.add_argument('--source', required=True)
parser.add_argument('--bun', required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--runs', type=positive_integer, default=12)
parser.add_argument('--case', type=probe_case, action='append', help='MODE:SCALE; repeat to override the default cases')
parser.add_argument('--paired-controls', action='store_true', help='Compare two identical-copy labels per build in each round, without Bun')
args = parser.parse_args()
commands = {'before': [args.before], 'after': [args.after], 'bun': [args.bun, args.source]}
if args.paired_controls:
    commands = {'before': [args.before], 'after': [args.after],
                'before_control': [args.before], 'after_control': [args.after]}
runner = args.output.with_suffix('.runner')
rows = []
cases = args.case or [
    ('all', 1), ('construct', 10), ('test-ascii', 100),
    ('test-emoji', 100), ('stripAnsi', 100), ('stripAnsi-match', 100)]
for mode, scale in cases:
    row = {'mode': mode, 'scale': scale, 'orders': [],
           'samples': {label: [] for label in commands}}
    expected = None
    for iteration in range(args.runs):
        order = list(commands)
        if args.paired_controls:
            order = [('before', 'after', 'after_control', 'before_control'),
                     ('after', 'before', 'before_control', 'after_control'),
                     ('before_control', 'after_control', 'after', 'before'),
                     ('after_control', 'before_control', 'before', 'after')][iteration % 4]
        else:
            # Rotation plus reversal covers all six orders; reversal alone
            # would always run the candidate in the middle of three builds.
            offset = iteration % len(order)
            order = order[offset:] + order[:offset]
            if iteration % 2:
                order.reverse()
        row['orders'].append(order)
        for label in order:
            command = commands[label]
            if label != 'bun':
                # Startup layout can affect allocation/GC inside a timed loop.
                # Keep argv[0], execPath, pathname and inode identical.
                shutil.copyfile(command[0], runner)
                runner.chmod(0o755)
                command = [str(runner)]
            result = subprocess.run(command + [mode, str(scale)], check=True,
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
