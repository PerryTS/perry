#!/usr/bin/env python3
"""tscwall: isolated parity, THP-off A/B, and locked main-thread counters.

Builds are external. All artifacts are beneath --hostdir. Run compile/compare
on CPUs 0-23,32-55; run wall inside MEASURE.lock on CPUs 56-63, setarch -R.
"""
import argparse
import ctypes
import fcntl
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time

import perf_objops_lane as common

ROOT = None
# Match scoreboard e's performance oracle; gap tests retain .node-version.
common.NODE[0] = '/opt/node-v26.8.1-linux-x64/bin/node'
MICROS = ('class_getter', 'object_array', 'class_virtual', 'object_spread')
common.PROGRAMS.update({
    'binarytrees_plain': ('binarytrees_plain.ts', ['21']),
    'binarytrees_arena': ('binarytrees_arena.ts', ['21']),
    **{n: ('micros/' + n + '.ts', ['5000000']) for n in MICROS},
})


def normalize(name, output):
    output = re.sub(rb'^(?:ms |elapsed_ms:).*\n', b'', output, flags=re.M)
    return common.normalized(name, output)


def command(name, arm, args=None):
    source, default = common.PROGRAMS[name]
    if arm == 'node':
        source = ROOT / 'programs' / source
        if name in MICROS:
            source = source.with_suffix('.js')
        return [*common.NODE, str(source), *(default if args is None else args)]
    return [str(ROOT / 'measure' / arm / name), *(default if args is None else args)]


def run(name, arm, prefix, args=None, events='instructions:u', main=False, gc=False, metrics=None):
    env = common.environment(ROOT, arm)
    if gc:
        env.update(PERRY_GC_DIAG='1', PERRY_GC_TRACE='1')
    source, _ = common.PROGRAMS[name]
    cmd = command(name, arm, args)
    if not gc:
        cmd = ['perf', 'stat', '-x', ';', '-e', events, '-o', str(prefix) + '.stat',
               *(['--no-inherit'] if main else []),
               *(['-M', metrics] if metrics else []), *cmd]
    cmd = ['/usr/bin/time', '-f', '%M;%e;%U;%S', '-o', str(prefix) + '.time', *cmd]
    result = common.execute(cmd, common.program_cwd(ROOT, source), env, prefix, True)
    oracle_path = ROOT / 'measure' / (name + (('-' + '-'.join(args)) if args else '') + '.node.out')
    if not oracle_path.exists():
        oracle_path.write_bytes(subprocess.check_output(command(name, 'node', args),
                                                       cwd=common.program_cwd(ROOT, source)))
    expected = oracle_path.read_bytes()
    assert result.returncode == 0 and normalize(name, result.stdout) == normalize(name, expected), (name, arm, result.returncode)
    rss, wall, user, system = map(float, Path(str(prefix) + '.time').read_text().strip().split(';'))
    row = dict(rss_kb=rss, wall_s=wall, user_s=user, sys_s=system)
    if not gc:
        for line in Path(str(prefix) + '.stat').read_text().splitlines():
            fields = line.split(';')
            if len(fields) >= 3 and re.fullmatch(r'[0-9.]+', fields[0]):
                row[fields[2]] = float(fields[0])
        if 'duration_time' in row:
            row['wall_s'] = row['duration_time'] / 1e9
    else:
        cycles = [json.loads(line) for line in result.stderr.splitlines() if line.startswith(b'{')]
        row['gc'] = [c for c in cycles if c.get('event') == 'gc_cycle']
        gc_lines = re.findall(rb'\[gc-time\].*', result.stderr)
        row['gc_time'] = gc_lines[-1].decode() if gc_lines else ''
        row.update({'gc_' + key: int(value) for key, value in
                    re.findall(r'(\w+)=(\d+)', row['gc_time'])})
        for kind in ('minor', 'full'):
            row['gc_' + kind + '_count'] = sum(c.get('collection_kind') == kind for c in row['gc'])
        if not row['gc']:
            # Minimal auto-optimized runtimes retain the light diagnostics
            # even when the full JSON trace feature is absent.
            row['gc_minor_count'] = len(re.findall(rb'^\[gc-copy-minor\] ran ', result.stderr, re.M))
            row['gc_full_count'] = len(re.findall(rb'^\[gc-full\] ', result.stderr, re.M))
        minor_lines = re.findall(rb'^\[gc-copy-minor\] ran .*', result.stderr, re.M)
        row['gc_minor_pause_sum_us'] = sum(int(re.search(rb'pause_us=(\d+)', line)[1])
                                           for line in minor_lines)
        row['gc_copy_and_remembered_us'] = sum(
            int(re.search(rb'copy_evacuation=(\d+)', line)[1])
            + int(re.search(rb'remembered_set_young_logs=(\d+)', line)[1])
            for line in minor_lines)
    return row


def siblings():
    return {int(f[0][3:]): list(map(int, f[1:])) for line in Path('/proc/stat').read_text().splitlines()
            if (f := line.split()) and f[0] in {'cpu' + str(n) for n in range(24, 32)}}


def compile_set(names, arms):
    path = ROOT / 'measure/status.json'
    result = json.loads(path.read_text()) if path.exists() else {}
    for name in names:
        source, args = common.PROGRAMS[name]
        cwd = common.program_cwd(ROOT, source)
        expected = subprocess.check_output(command(name, 'node'), cwd=cwd)
        (ROOT / 'measure' / (name + '.node.out')).write_bytes(expected)
        result.setdefault(name, {})
        for arm in arms:
            env = common.environment(ROOT, arm)
            if name not in ('binarytrees_plain', 'binarytrees_arena'):
                env.pop('PERRY_NO_AUTO_OPTIMIZE', None)
            binary = ROOT / 'measure' / arm / name
            compiled = common.execute([str(Path(env['CARGO_TARGET_DIR']) / 'release/perry'),
                                       'compile', str(ROOT / 'programs' / source),
                                       '--keep-intermediates', '-o', str(binary)], cwd, env,
                                      binary.with_suffix('.compile'), True)
            status = {'status': 'compile-fail', 'exit': compiled.returncode}
            if compiled.returncode == 0:
                checked = common.execute([str(binary), *args], cwd, env,
                                         binary.with_suffix('.check'), True)
                emitted = subprocess.check_output(['size', '-A', str(binary)], text=True)
                status = {'status': 'pass' if checked.returncode == 0 and
                          normalize(name, checked.stdout) == normalize(name, expected)
                          else 'output-fail', 'exit': checked.returncode,
                          'text': int(re.search(r'^\.text\s+(\d+)', emitted, re.M)[1]),
                          'auto_optimize': 'PERRY_NO_AUTO_OPTIMIZE' not in env}
            print(name, arm, status, flush=True)
            with (ROOT / 'measure/status.lock').open('w') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX)
                result = json.loads(path.read_text()) if path.exists() else {}
                result.setdefault(name, {})[arm] = status
                path.write_text(json.dumps(result, indent=2))


def measure(names, arms, action, trials=5, thp_off=False):
    if (action == 'compare' or thp_off) and ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError('PR_SET_THP_DISABLE failed')
    path = ROOT / 'measure' / (action + '.json')
    result = json.loads(path.read_text()) if path.exists() else {}
    for name in names:
        before = siblings()
        rows = {arm: [] for arm in arms}
        for trial in range(trials):
            for arm in arms if trial % 2 == 0 else reversed(arms):
                prefix = ROOT / 'measure/trials' / f'{action}-{name}-{arm}-{trial}'
                if action == 'wall':
                    for group, events in enumerate((
                        'duration_time,cycles,instructions,instructions:u,cycles:k,task-clock,page-faults,context-switches',
                        'duration_time,instructions,ic_tag_hit_miss.instruction_cache_miss,bp_l1_tlb_miss_l2_tlb_hit,bp_l1_tlb_miss_l2_tlb_miss.all,op_cache_hit_miss.all_op_cache_accesses,op_cache_hit_miss.op_cache_miss',
                        'duration_time,instructions,cycles',
                    )):
                        rows[arm].append(run(name, arm, Path(str(prefix) + f'-{group}'), events=events,
                                             main=arm == 'node-main', metrics='PipelineL1' if group == 2 else None))
                else:
                    rows[arm].append(run(name, arm, prefix, gc=action == 'gc'))
                    if action == 'compare' and name in MICROS:
                        short = run(name, arm, Path(str(prefix) + '-short'), args=['1000000'])
                        rows[arm][-1]['short_instructions:u'] = short['instructions:u']
        after = siblings()
        sib = {}
        for cpu, first in before.items():
            delta = [b - a for a, b in zip(first, after[cpu])]
            sib[cpu] = 100 * (sum(delta[:8]) - delta[3] - delta[4]) / max(1, sum(delta[:8]))
        result[name] = {'rows': rows, 'sibling_util_pct': sib}
        path.write_text(json.dumps(result, indent=2))
        print(name, action, {a: {k: statistics.median(r[k] for r in rs if k in r)
              for k in rs[0] if isinstance(rs[0][k], (int, float))} for a, rs in rows.items()}, sib, flush=True)


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--hostdir', type=Path, required=True)
    p.add_argument('--programs', default=','.join(common.PROGRAMS))
    p.add_argument('--arms', default='main,head')
    p.add_argument('--trials', type=int, default=5)
    p.add_argument('--thp-off', action='store_true')
    p.add_argument('action', choices=('compile', 'compare', 'wall', 'gc'))
    opts = p.parse_args()
    ROOT = opts.hostdir.resolve()
    # node-main uses the same command but restricts perf inheritance.
    original_command = command
    command = lambda name, arm, args=None: original_command(name, 'node' if arm == 'node-main' else arm, args)
    for d in ('measure/main', 'measure/head', 'measure/node', 'measure/node-main', 'measure/trials', 'tmp'):
        (ROOT / d).mkdir(parents=True, exist_ok=True)
    if opts.action == 'compile':
        compile_set(opts.programs.split(','), opts.arms.split(','))
    else:
        measure(opts.programs.split(','), opts.arms.split(','), opts.action, opts.trials, opts.thp_off)
