#!/usr/bin/env python3
"""Object-array lane: isolated builds, Node parity, THP-off n=5 A/B.

Run on qb6 with taskset -c 0-23,32-55 setarch -R. This script neither
builds nor installs anything; all outputs stay beneath --hostdir.
The control arm compiles with the existing PERRY_REGION_ELEMENTS=0 knob.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import re
import statistics
import subprocess

import perf_objops_lane as common

MICROS = ('object_array', 'class_getter', 'class_virtual', 'object_spread')


def normalized(output):
    return re.sub(rb'^elapsed_ms:.*\n', b'', output, flags=re.M)


def micros(root, arms, names, node_single_threaded, node_controlled, node_eager_turbofan, measure_existing):
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError('PR_SET_THP_DISABLE failed')
    path = root / 'measure/class-micros.json'
    results = json.loads(path.read_text()) if path.exists() else {}
    wrapper = root / 'measure/node-control.cjs'
    wrapper.write_text("""const fs = require('node:fs');
const ctl = fs.openSync(process.env.PERF_MICRO_CONTROL, 'w');
const ack = fs.openSync(process.env.PERF_MICRO_ACK, 'r');
const original = console.log;
function command(text) {
  fs.writeSync(ctl, text + '\\n');
  const byte = Buffer.alloc(1);
  while (fs.readSync(ack, byte, 0, 1, null) && byte[0] !== 10) {}
}
console.log = function (...args) {
  if (args[0] === 'measure:end') command('disable');
  original.apply(console, args);
  if (args[0] === 'measure:begin') {
    // Let the prescribed warmup's background compilation finish before the
    // counter interval. Neither the source nor its warmup count changes.
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 200);
    command('enable');
  }
};
""")
    for name in names:
        source = root / 'programs/micros' / f'{name}.ts'
        cwd = source.parent
        expected = {}
        for n in (50000, 200000):
            expected[n] = common.execute(
                ['node', str(source.with_suffix('.js')), str(n)], cwd,
                common.environment(root, 'head'),
                root / 'measure' / f'{name}.node.{n}').stdout
        row = results.setdefault(name, {})
        for arm in arms:
            if arm != 'node':
                if not measure_existing:
                    previous = os.environ.get('PERRY_REGION_ELEMENTS')
                    try:
                        if arm == 'control':
                            os.environ['PERRY_REGION_ELEMENTS'] = '0'
                        row[arm] = common.compile_one(root, arm, name, source, cwd,
                                                      expected[200000], ['200000'])
                    finally:
                        if previous is None:
                            os.environ.pop('PERRY_REGION_ELEMENTS', None)
                        else:
                            os.environ['PERRY_REGION_ELEMENTS'] = previous
                else:
                    assert row[arm]['status'] == 'pass', (name, arm)
                assert row[arm]['status'] != 'compile-fail' and row[arm]['exit'] == 0, (name, arm, row[arm])
                # The shared comparator does not strip the timing line.
                checked = (root / 'measure' / arm / f'{name}.check.out').read_bytes()
                row[arm]['status'] = 'pass' if normalized(checked) == normalized(expected[200000]) else 'output-fail'
                assert row[arm]['status'] == 'pass', (name, arm, row[arm])
            else:
                row[arm] = {'status': 'pass'}
        trials = {arm: [] for arm in arms}
        for trial in range(5):
            for arm in arms if trial % 2 == 0 else reversed(arms):
                counts = {}
                rss = {}
                for n in (50000, 200000):
                    prefix = root / 'measure/trials' / f'{name}-{arm}-{trial}-{n}'
                    node_flags = ['--single-threaded'] if node_single_threaded else []
                    if node_eager_turbofan:
                        node_flags += ['--no-maglev', '--invocation-count-for-turbofan=1', '--no-concurrent-recompilation']
                    env = common.environment(root, arm)
                    perf_flags = []
                    if arm == 'node' and node_controlled:
                        # Whole-process Node counts are dominated by variable
                        # startup/JIT work at these small Ns. Count only the
                        # existing marker interval, with synchronous perf ACKs;
                        # the documented two-N subtraction still removes its
                        # fixed setup and marker overhead.
                        node_flags += ['--require', str(wrapper)]
                        control, ack = prefix.with_suffix('.control'), prefix.with_suffix('.ack')
                        for fifo in (control, ack):
                            fifo.unlink(missing_ok=True)
                            os.mkfifo(fifo)
                        env.update(PERF_MICRO_CONTROL=str(control), PERF_MICRO_ACK=str(ack))
                        perf_flags = ['-D', '-1', '--control', f'fifo:{control},{ack}']
                    command = ['node', *node_flags, str(source.with_suffix('.js'))] if arm == 'node' else [str(root / 'measure' / arm / name)]
                    result = common.execute(
                        ['perf', 'stat', *perf_flags, '-x', ';', '-e', 'instructions:u', '-o', str(prefix.with_suffix('.stat')),
                         '/usr/bin/time', '-f', '%M', '-o', str(prefix.with_suffix('.rss')), *command, str(n)],
                        cwd, env, prefix)
                    assert normalized(result.stdout) == normalized(expected[n]), (name, arm, n)
                    counts[n] = int(re.search(r'^(\d+);;instructions:u;', prefix.with_suffix('.stat').read_text(), re.M)[1])
                    rss[n] = int(prefix.with_suffix('.rss').read_text().strip())
                trials[arm].append({'instructions_per_iteration': (counts[200000] - counts[50000]) / 150000,
                                    'instructions_200k': counts[200000], 'rss_kb': rss[200000]})
        for arm in arms:
            row[arm]['trials'] = trials[arm]
            row[arm]['median'] = {key: statistics.median(t[key] for t in trials[arm]) for key in trials[arm][0]}
            if arm == 'node':
                row[arm]['single_threaded'] = node_single_threaded
                row[arm]['marker_controlled'] = node_controlled
                row[arm]['eager_turbofan'] = node_eager_turbofan
        path.write_text(json.dumps(results, indent=2))
        print(name, {a: row[a] for a in arms}, flush=True)


def gaps(root, arms):
    names = ('objarray_numeric_region', 'region_array_facts',
             'region_module_class_elements', 'masked_region_number_scope',
             'typed_array_view_region', 'typed_array_length_bound_region')
    status = {}
    for name in names:
        source = root / 'src/test-files' / f'test_gap_{name}.ts'
        expected = common.oracle(root, name, source, [], source.parent)
        status[name] = {}
        for arm in arms:
            row = common.compile_one(root, arm, name, source, source.parent, expected, [])
            status[name][arm] = row
            print('gap', name, arm, row, flush=True)
    (root / 'measure/gaps.json').write_text(json.dumps(status, indent=2))


def micro_gc(root, names, arms):
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError('PR_SET_THP_DISABLE failed')
    path = root / 'measure/micro-gc.json'
    counts = json.loads(path.read_text()) if path.exists() else {}
    for name in names:
        counts.setdefault(name, {})
        for arm in (a for a in arms if a != 'node'):
            env = common.environment(root, arm)
            env.update(PERRY_GC_DIAG='1', PERRY_GC_TRACE='1')
            result = common.execute([str(root / 'measure' / arm / name), '200000'],
                                    root / 'programs/micros', env,
                                    root / 'measure' / arm / f'{name}.gc')
            cycles = {'full': 0, 'minor': 0}
            for line in result.stderr.splitlines():
                if line.startswith(b'{'):
                    event = json.loads(line)
                    if event.get('event') == 'gc_cycle':
                        kind = event['collection_kind']
                        cycles[kind] = cycles.get(kind, 0) + 1
            counts[name][arm] = cycles
            print('GC micro', name, arm, cycles, flush=True)
    path.write_text(json.dumps(counts, indent=2))


def boundary(root):
    source = root / 'src/test-files/test_gap_objarray_large_bound.ts'
    expected = common.oracle(root, 'objarray-large-bound', source, [], source.parent)
    results = {}
    for arm in ('main', 'head'):
        env = common.environment(root, arm)
        binary = root / 'measure' / arm / 'objarray-large-bound'
        common.execute([str(Path(env['CARGO_TARGET_DIR']) / 'release/perry'),
                        'compile', str(source), '-o', str(binary)], source.parent,
                       env, binary.with_suffix('.compile'))
        try:
            result = subprocess.run([str(binary)], cwd=source.parent, env=env,
                                    capture_output=True, timeout=5)
            results[arm] = {'exit': result.returncode,
                            'pass': result.returncode == 0 and result.stdout == expected,
                            'stdout': result.stdout.decode()}
        except subprocess.TimeoutExpired:
            # subprocess.run terminates only the binary it started.
            results[arm] = {'timeout_seconds': 5, 'pass': False}
        print('boundary', arm, results[arm], flush=True)
    (root / 'measure/boundary.json').write_text(json.dumps(results, indent=2))
    assert results['head']['pass'], results


def stress(root):
    name = 'objarray_numeric_region'
    source = root / 'src/test-files' / f'test_gap_{name}.ts'
    expected = common.oracle(root, name, source, [], source.parent)
    results = {}
    for seed in (1, 7):
        env = common.environment(root, 'head')
        env.update(PERRY_GC_SCHEDULE_SEED=str(seed), PERRY_GC_SCHEDULE_RATE='1',
                   PERRY_GC_SCHEDULE_ALLOC_KB='0', PERRY_GC_PROTECT_FROMSPACE='1',
                   PERRY_GC_PROTECT_FROMSPACE_DEPTH='16', PERRY_GC_VERIFY_EVACUATION='1',
                   PERRY_GC_DIAG='1', PERRY_GC_TRACE='1')
        result = common.execute([str(root / 'measure/head' / name)], source.parent,
                                env, root / 'measure' / f'gap-stress-{seed}', allow_fail=True)
        copied = sum(map(int, re.findall(rb'copied_objects=(\d+)', result.stderr)))
        retired = re.findall(rb'retired_set=#(\d+)', result.stderr)
        passed = result.returncode == 0 and result.stdout == expected and copied > 0 and bool(retired)
        results[seed] = {'pass': passed, 'exit': result.returncode,
                         'copied_objects': copied, 'retired_sets': len(retired)}
        print('stress', seed, results[seed], flush=True)
    (root / 'measure/gc-stress.json').write_text(json.dumps(results, indent=2))
    assert all(r['pass'] for r in results.values()), results


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--arms', default='main,head,node')
    parser.add_argument('--programs', default=','.join(common.PROGRAMS))
    parser.add_argument('--micros', default=','.join(MICROS))
    parser.add_argument('--measure-existing', action='store_true')
    parser.add_argument('--node-single-threaded', action='store_true')
    parser.add_argument('--node-controlled', action='store_true')
    parser.add_argument('--node-eager-turbofan', action='store_true')
    parser.add_argument('action', choices=('micros', 'compile', 'compare', 'gc', 'gaps', 'micro-gc', 'stress', 'boundary'))
    opts = parser.parse_args()
    root = opts.hostdir.resolve()
    for directory in ('tmp', 'measure/main', 'measure/head', 'measure/trials'):
        (root / directory).mkdir(parents=True, exist_ok=True)
    for arm in opts.arms.split(','):
        if arm != 'node':
            (root / 'measure' / arm).mkdir(parents=True, exist_ok=True)
    if opts.action == 'boundary':
        boundary(root)
    elif opts.action == 'micros':
        micros(root, opts.arms.split(','), opts.micros.split(','), opts.node_single_threaded, opts.node_controlled, opts.node_eager_turbofan, opts.measure_existing)
    elif opts.action == 'stress':
        stress(root)
    elif opts.action == 'gaps':
        gaps(root, opts.arms.split(','))
    elif opts.action == 'micro-gc':
        micro_gc(root, opts.micros.split(','), opts.arms.split(','))
    elif opts.action == 'compile':
        common.compile_set(root, opts.programs.split(','), arms=tuple(a for a in opts.arms.split(',') if a != 'node'))
    elif opts.action == 'compare':
        common.compare(root, opts.programs.split(','))
    else:
        path = root / 'measure/gc-counts.json'
        previous = json.loads(path.read_text()) if path.exists() else {}
        common.gc(root, opts.programs.split(','))
        previous.update(json.loads(path.read_text()))
        path.write_text(json.dumps(previous, indent=2))
