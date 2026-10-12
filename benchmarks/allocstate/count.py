#!/usr/bin/env python3
"""Instruction attribution at two smaller binary-trees depths; full workload is measured separately."""
import argparse
import importlib.util
from pathlib import Path
import re
import subprocess

spec = importlib.util.spec_from_file_location('common', Path(__file__).resolve().parents[2] / 'scripts/perf_objops_lane.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
p = argparse.ArgumentParser()
p.add_argument('--hostdir', type=Path, required=True)
p.add_argument('--arm', default='main')
a = p.parse_args()
root = a.hostdir
out = root/'count'/a.arm
out.mkdir(parents=True, exist_ok=True)
for directory in ('tmp', 'measure/main', 'measure/head'):
    (root/directory).mkdir(parents=True, exist_ok=True)
env = m.environment(root, a.arm)
env['PERRY_TARGET_CPU'] = 'x86-64-v3'
import os
os.environ['PERRY_TARGET_CPU'] = 'x86-64-v3'
source = (root/'programs/binarytrees_plain.ts').read_text()
for depth in (11,12):
    ts = root/'programs'/f'plain-count-{depth}.ts'
    ts.write_text(source.replace('const maxDepth = 21;', f'const maxDepth = {depth};'))
    binary = out/f'plain-{depth}'
    expected = m.oracle(root, f'plain-{depth}', ts, [], ts.parent)
    status = m.compile_one(root, a.arm, f'count-{depth}', ts, ts.parent, expected, [])
    compiled = root/'measure'/a.arm/f'count-{depth}'
    compiled.rename(binary)
    if status['status'] != 'pass':
        raise RuntimeError(status)
    m.execute(['valgrind','--tool=callgrind','--dump-instr=yes','--aspace-minaddr=0x100000000',
               '--callgrind-out-file='+str(out/f'plain-{depth}.cg'),str(binary)], ts.parent, env,
              out/f'plain-{depth}.profile')
    report = subprocess.check_output(['callgrind_annotate','--inclusive=yes','--auto=no','--threshold=100','--show-percs=no',str(out/f'plain-{depth}.cg')],text=True)
    (out/f'plain-{depth}.inclusive.txt').write_text(report)
    self_report = subprocess.check_output(['callgrind_annotate','--inclusive=no','--auto=no','--threshold=100','--show-percs=no',str(out/f'plain-{depth}.cg')],text=True)
    (out/f'plain-{depth}.self.txt').write_text(self_report)
    print(depth, status, flush=True)
    print('\n'.join(l for l in report.splitlines() if any(s in l for s in ('TOTALS','perry_birth_class','js_inline_arena_state','js_write_barrier_slot','js_gc_note_black_birth','ValidPointerSet'))), flush=True)
