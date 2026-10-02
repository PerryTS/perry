#!/usr/bin/env python3
"""Rotate matched executables; check Node output; collect wait4 CPU/peak RSS.

Usage: python3 measure.py BASELINE_DIR CANDIDATE_DIR OUTPUT_DIR
Each input directory contains executables named cyclic and growing.
Requires macOS (ru_maxrss is bytes) and the repository's pinned Node on PATH.
"""
import json
import os
from pathlib import Path
import resource
import subprocess
import sys

baseline, candidate, output = map(lambda arg: Path(arg).resolve(), sys.argv[1:])
output.mkdir(parents=True, exist_ok=True)
sources = Path(__file__).resolve().parent
if sys.platform != 'darwin':
    raise RuntimeError('This harness records macOS wait4 RSS in bytes')
node_version = subprocess.check_output(['node', '--version'], text=True).strip().removeprefix('v')
expected_version = (sources.parents[1] / '.node-version').read_text().strip()
if node_version != expected_version:
    raise RuntimeError(f'Node {expected_version} is required; found {node_version}')
workloads = [('cyclic', [])] + [
    ('growing', [str(width), str(batches)])
    for width, batches in [(4, 1000000), (5, 1000000), (16, 1000000), (100000, 100)]
]
rows = []
for workload, args in workloads:
    name = workload + ('-' + args[0] if args else '')
    expected = subprocess.check_output(['node', str(sources / (workload + '.ts')), *args])
    for repeat in range(3):
        arms = [('baseline', baseline), ('candidate', candidate)]
        if repeat % 2:
            arms.reverse()
        for arm, directory in arms:
            stdout = output / f'{name}-{arm}-{repeat}.stdout'
            stderr = output / f'{name}-{arm}-{repeat}.stderr'
            binary = directory / workload
            pid = os.fork()
            if pid == 0:
                os.environ['PERRY_GC_HEAP_LIMIT'] = '1024'
                with stdout.open('wb') as f:
                    os.dup2(f.fileno(), 1)
                with stderr.open('wb') as f:
                    os.dup2(f.fileno(), 2)
                os.execv(str(binary), [str(binary), *args])
            _, status, usage = os.wait4(pid, 0)
            if status != 0 or stdout.read_bytes() != expected:
                raise RuntimeError(f'{name}/{arm}: status={status}; check {stdout} and {stderr}')
            row = dict(workload=name, arm=arm, repeat=repeat,
                       cpu_s=usage.ru_utime + usage.ru_stime,
                       rss_mib=usage.ru_maxrss / 1024**2)
            rows.append(row)
            print(json.dumps(row), flush=True)
(output / 'measurements.json').write_text(json.dumps(rows, indent=2) + '\n')
