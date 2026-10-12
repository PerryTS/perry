#!/usr/bin/env python3
"""Lane-local extension of the common program A/B harness (qb6 only)."""
import argparse
import ast
import ctypes
import importlib.util
import json
from pathlib import Path
import re
import statistics
import sys
import os
import shutil
os.environ['PERRY_TARGET_CPU'] = 'x86-64-v3'

spec = importlib.util.spec_from_file_location('common', Path(__file__).resolve().parents[2] / 'scripts/perf_objops_lane.py')
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)
common.PROGRAMS.update({
    'class_getter': ('micros/class_getter.ts', ['200000']),
    'object_array': ('micros/object_array.ts', ['200000']),
    'class_virtual': ('micros/class_virtual.ts', ['200000']),
    'object_spread': ('micros/object_spread.ts', ['200000']),
    'binarytrees_plain': ('binarytrees_plain.ts', []),
    'binarytrees_arena': ('binarytrees_arena.ts', []),
    'root-survival': ('root-survival.ts', []),
})
original_oracle = common.oracle
def oracle(root, name, source, args, cwd):
    if source.parent.name == 'micros':
        source = source.with_suffix('.js')
    return original_oracle(root, name, source, args, cwd)
common.oracle = oracle
original_normalize = common.normalized
def normalized(name, output):
    return re.sub(rb'^elapsed_ms.*\n', b'', original_normalize(name, output), flags=re.M)
common.normalized = normalized

p = argparse.ArgumentParser()
p.add_argument('--hostdir', required=True, type=Path)
p.add_argument('--arms', default='main,head')
p.add_argument('--programs', default=','.join(common.PROGRAMS))
p.add_argument('action', choices=['compile', 'compare', 'gc', 'micro-low'])
a = p.parse_args()
root = a.hostdir.resolve()
for directory in ('tmp', 'measure/main', 'measure/head', 'measure/trials'):
    (root/directory).mkdir(parents=True, exist_ok=True)
names = a.programs.split(',')
# Compiler arms can finish at different times. Reconstruct the combined status
# from each arm's append-only log rather than let a stale in-memory dictionary
# erase the other arm's completed results.
if a.action != 'compile':
    status = {}
    for log in ('compile-trees-main.log', 'compile-main.log', 'compile-head.log', 'compile-trees-head-final.log', 'compile-fastify-final.log'):
        path = root/log
        if not path.exists(): continue
        for line in path.read_text().splitlines():
            match = re.fullmatch(r'(\S+) (main|head) (\{.*\})', line)
            if match:
                status.setdefault(match[1], {})[match[2]] = ast.literal_eval(match[3])
    (root/'measure/status.json').write_text(json.dumps(status, indent=2))
if a.action == 'compile':
    common.compile_set(root, names, arms=a.arms.split(','))
elif a.action == 'compare':
    common.compare(root, names)
elif a.action == 'gc':
    common.gc(root, names)
else:
    micros = ['class_getter', 'object_array', 'class_virtual', 'object_spread']
    high = root/'measure/results-thp-off.json'
    saved_results = high.read_bytes()
    expected = {name: (root/'measure'/f'{name}.node.out').read_bytes() for name in micros}
    high.unlink()
    backup = root/'measure/micro-high-trials'
    backup.mkdir(exist_ok=True)
    for name in micros:
        for path in (root/'measure/trials').glob(name+'-*'):
            shutil.copy2(path, backup/path.name)
    try:
        for name in micros:
            common.PROGRAMS[name] = (common.PROGRAMS[name][0], ['50000'])
            rel, args = common.PROGRAMS[name]
            source = root/'programs'/rel
            common.oracle(root, name, source, args, source.parent)
        common.compare(root, micros)
        high.rename(root/'measure/micro-low-results.json')
    finally:
        high.write_bytes(saved_results)
        for name, output in expected.items():
            (root/'measure'/f'{name}.node.out').write_bytes(output)
