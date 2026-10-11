#!/usr/bin/env python3
"""Compare fresh native-root populations with the unchanged zero budgets."""
import concurrent.futures
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(os.environ['KEYLISTS_ROOT'])
E = ROOT / 'evidence2'
CPUS = '0-23,32-55'
if os.environ.get('KEYLISTS_WAIT_BUILD')=='1':
    name=os.environ['KEYLISTS_ROUND_NAME']
    head=ROOT/f'keylists2-{name}-head-build.log'
    frontend=ROOT/f'keylists2-{name}-frontend.log'
    while not head.exists() or 'Finished ' not in head.read_text() or not frontend.exists():
        time.sleep(10)


def checked(argv, tree, env, log, required=True):
    with (E / log).open('w') as output:
        result = subprocess.run(['taskset', '-c', CPUS, *argv], cwd=tree,
                                env=env, stdout=output, stderr=subprocess.STDOUT)
    if required:
        assert result.returncode == 0, (argv, result.returncode, log)
    return result.returncode


def corpus(arm):
    base = ROOT if arm == 'head' else ROOT / 'main-current'
    target = ROOT / ('native-' + arm + '-target')
    assert not target.exists(), target
    subprocess.run(['cp', '-a', '--reflink=auto', str(base / 'target'),
                    str(target)], check=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(target),
               PERRY_RUNTIME_DIR=str(target / 'release'),
               PERRY_WORKSPACE_ROOT=str(base / 'src'),
               PERRY_BIN=str(base / 'target/release/perry'),
               PERRY_LLVM_OPT='/usr/lib/llvm-22/bin/opt',
               PERRY_MODULE_JOBS='1', PERRY_CODEGEN_UNIT_JOBS='1',
               CARGO_BUILD_JOBS='1', MIN_SOURCES='317', MAX_SKIPPED='0',
               PERRY_NO_CACHE='1')
    checked(['cargo', 'build', '--release', '-j1', '-p', 'perry-ext-net'],
            base / 'src', env, 'native-' + arm + '-libraries.log')
    shim = ROOT / 'tmp/native-no-build-bin'
    shim.mkdir(exist_ok=True)
    refusal = shim / 'cargo'
    refusal.write_text('#!/bin/sh\necho "optional corpus build refused" >&2\nexit 1\n')
    refusal.chmod(0o755)
    env['PATH'] = str(shim) + ':' + env['PATH']
    path = E / ('native-' + arm)
    checked(['bash', 'scripts/gc_root_dominance_corpus.sh', str(path),
             '--lowering', 'native'], base / 'src', env,
            'native-' + arm + '-corpus.log')
    return path


running=ROOT/'tmp/native-current.running'
assert not running.exists()
running.write_text('two native jobs reserved\n')
try:
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        paths = dict(zip(['main', 'head'], pool.map(corpus, ['main', 'head'])))

    checker = ROOT / 'src/scripts/gc_root_dominance_check.py'
    ns = {'__name__': 'native_census_reader', '__file__': str(checker)}
    exec(compile(checker.read_text(), str(checker), 'exec'), ns)
    counts = {}
    for arm, path in paths.items():
        sums = dict(files=0, funcs=0, statepoints=0, live_bundles=0, relocates=0)
        for file in sorted(path.glob('*.ll')):
            functions = ns['parse_file'](str(file))
            points, live, relocates, _, unreadable = ns['statepoint_corpus_stats'](
                [(file.name, functions)])
            assert not unreadable, file
            sums['files'] += 1
            sums['funcs'] += len(functions)
            sums['statepoints'] += points
            sums['live_bundles'] += live
            sums['relocates'] += relocates
        counts[arm] = sums
    assert counts['main'] == counts['head'], counts
    assert all(value > 0 for value in counts['main'].values()), counts
    flags = ['--statepoints', '--moving-only', '--max-unrooted', '0',
             '--max-stale', '0']
    for key, value in counts['main'].items():
        flags += ['--min-' + key.replace('_', '-'), str(value)]
    statuses = {}
    for arm, path in paths.items():
        extra = ['--seeded-violations', '40'] if arm == 'head' else []
        statuses[arm] = checked(
            ['python3', str(ROOT / 'src/lane-keylists/root-check-indexed.py'),
             str(path), *flags, *extra], ROOT / 'src', dict(os.environ),
            'native-' + arm + '-gate.log', required=False)
    (E / 'native-cohort.json').write_text(json.dumps(
        dict(counts=counts, statuses=statuses, zero_budgets_retained=True), indent=2) + '\n')
    print('Matched native corpus:', counts, statuses, flush=True)
finally:
    running.unlink()
