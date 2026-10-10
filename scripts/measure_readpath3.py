#!/usr/bin/env python3
"""Pinned, off-lock readpath3 instruction/RSS A/B; builds happen separately."""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import threading

PROGRAMS = {
    'effect': ('programs', 'effect.ts', []),
    'schema-record': ('programs', 'schema-record.ts', []),
    'property-prototype': ('programs', 'property-prototype.ts', []),
    'shapes-options': ('programs', 'shapes-options.ts', []),
    'fastify': ('realprog/pk', 'fastify/inject.ts', ['5000', '300']),
    'qs-parse': ('realprog/pk', 'qs/parse_nested.ts', ['20000', '1000']),
    'qs-stringify': ('realprog/pk', 'qs/stringify_nested.ts', ['20000', '1000']),
    'commander': ('realprog/pk', 'commander/parse_argv.ts', ['5000', '200']),
    'tsc': ('realprog', 'tscwork.ts', ['3']),
    'zod5k': ('realprog', 'zodwork.ts', ['5000']),
    'hello': ('realprog', 'hello.ts', []),
    'buffer_heavy': ('drivers', 'buffer_heavy.ts', []),
    'worker_heavy': ('drivers', 'worker_heavy.ts', []),
}
OPTIONAL = {'buffer_heavy', 'worker_heavy'}
NODE = ['node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--experimental-strip-types']


def disable_thp():
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE')


def environment(root, arm, name):
    lane = root / 'main-arm' if arm == 'main' else root
    # The no-auto stream path can rebuild coherent archives when it finds
    # source. Consume only the separately built full archives in each arm.
    prebuilt = root / ('prebuilt-' + arm)
    for crate in ['perry-runtime', 'perry-ui-geisterhand']:
        (prebuilt / 'crates' / crate).mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(lane / 'target'),
               PERRY_RUNTIME_DIR=str(lane / 'target/release'),
               PERRY_WORKSPACE_ROOT=str(prebuilt), RUST_TEST_THREADS='1',
               CARGO_BUILD_JOBS='8', RAYON_NUM_THREADS='8', PERRY_SKIP_BUILD='1',
               PERRY_NO_AUTO_OPTIMIZE='1', PERRY_NO_CACHE='1',
               PERRY_ALLOW_PERRY_FEATURES='1', TMPDIR=str(root / 'tmp'),
               PERRY_MODULE_JOBS='1', PERRY_CODEGEN_UNIT_JOBS='8',
               PERRY_FORCE_WELL_KNOWN='http,net,ws,zlib')
    for key in ['PERRY_GC_DIAG', 'PERRY_GC_TRACE', 'PERRY_METHOD_SITE_STATS']:
        env.pop(key, None)
    if name == 'tsc':
        env['PERRY_LL_RS4GC_MAX_INSTRS'] = '2097152'
    return lane, env


def run(command, cwd, env, prefix, *, thp_off=False, timeout=1800):
    prefix.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(command, cwd=cwd, env=env, capture_output=True,
                            timeout=timeout, preexec_fn=disable_thp if thp_off else None)
    Path(str(prefix) + '.out').write_bytes(result.stdout)
    Path(str(prefix) + '.err').write_bytes(result.stderr)
    if result.returncode:
        raise RuntimeError(f'{prefix}: exit {result.returncode}: {result.stderr.decode(errors="replace")[-1200:]}')
    return result.stdout


def compile_arm(root, out, arm, names):
    lane, _ = environment(root, arm, names[0])

    def archives():
        return {filename: {
            'sha256': hashlib.sha256((lane / 'target/release' / filename).read_bytes()).hexdigest(),
            'mtime_ns': (lane / 'target/release' / filename).stat().st_mtime_ns,
        } for filename in ['perry', 'libperry_runtime.a', 'libperry_stdlib.a',
                           'libperry_ext_http.a', 'libperry_ext_net.a',
                           'libperry_ext_ws.a', 'libperry_ext_zlib.a']}

    before = archives()
    status = {}
    for name in names:
        directory, source, args = PROGRAMS[name]
        cwd = root / directory
        lane, env = environment(root, arm, name)
        prefix = out / arm / name
        try:
            run([str(lane / 'target/release/perry'), 'compile', '--no-auto-optimize',
                 source, '-o', str(prefix)], cwd, env, Path(str(prefix) + '.compile'), timeout=3600)
            expected = run([*NODE, source, *args], cwd, env, Path(str(prefix) + '.node'))
            actual = run([str(prefix), *args], cwd, env, Path(str(prefix) + '.perry'))
            if expected != actual:
                raise RuntimeError('Node output differs')
            status[name] = 'PASS'
        except Exception as error:
            status[name] = str(error)
        (out / arm / 'status.json').write_text(json.dumps(status, indent=2) + '\n')
        print(f'{arm}/{name}: {status[name]}', flush=True)
        if status[name] != 'PASS' and name not in OPTIONAL:
            raise RuntimeError(f'required program {name} failed')
    after = archives()
    (out / arm / 'provenance.json').write_text(json.dumps({'before': before, 'after': after}, indent=2) + '\n')
    if after != before:
        raise RuntimeError(f'{arm}: measured archives changed during driver compilation')


def gc_counts(command, cwd, env, prefix, expected, thp_off):
    diag = dict(env, PERRY_GC_TRACE='1')
    stop = threading.Event()
    samples = []
    proc = subprocess.Popen(command, cwd=cwd, env=diag, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, preexec_fn=disable_thp if thp_off else None)

    def sample():
        while not stop.is_set():
            try:
                values = {}
                for line in Path(f'/proc/{proc.pid}/smaps_rollup').read_text().splitlines():
                    key, _, value = line.partition(':')
                    if key in {'Rss', 'Anonymous', 'AnonHugePages'}:
                        values[key] = int(value.split()[0])
                samples.append(values)
            except (OSError, ValueError):
                pass
            stop.wait(0.02)

    sampler = threading.Thread(target=sample)
    sampler.start()
    try:
        stdout, stderr = proc.communicate(timeout=1800)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.communicate()
        raise
    finally:
        stop.set()
        sampler.join()
    Path(str(prefix) + '.out').write_bytes(stdout)
    Path(str(prefix) + '.err').write_bytes(stderr)
    if proc.returncode or stdout != expected or b'diagnostics feature disabled' in stderr:
        raise RuntimeError(f'{prefix}: diagnostic parity/features failed')
    events = []
    for line in stderr.decode(errors='replace').splitlines():
        if line.startswith('{'):
            event = json.loads(line)
            if event.get('event') == 'gc_cycle':
                events.append(event)
    assert all(e['collection_kind'] in {'full', 'minor'} for e in events)
    peak = max(samples, key=lambda s: s.get('Rss', 0)) if samples else {}
    return {'fulls': sum(e['collection_kind'] == 'full' for e in events),
            'minors': sum(e['collection_kind'] == 'minor' for e in events),
            'anon_huge_kib': max((s.get('AnonHugePages', 0) for s in samples), default=None),
            'sampled_anon_kib': peak.get('Anonymous'),
            'sampled_file_kib': peak['Rss'] - peak['Anonymous'] if peak else None}


def measure(root, out, names, thp_off, tag=''):
    status = {arm: json.loads((out / arm / 'status.json').read_text()) for arm in ['main', 'head']}
    suffix = ('-' + tag if tag else '') + ('-thp-off' if thp_off else '')
    folder = out / ('trials' + suffix)
    result_file = out / ('results' + suffix + '.json')
    results = json.loads(result_file.read_text()) if result_file.exists() else {}
    for name in names:
        if status['main'].get(name) != 'PASS':
            if name not in OPTIONAL:
                raise RuntimeError(f'{name}: baseline parity failed')
            print(f'{name}: optional baseline failure; excluded', flush=True)
            continue
        if status['head'].get(name) != 'PASS':
            raise RuntimeError(f'{name}: candidate parity failed')
        directory, _, args = PROGRAMS[name]
        cwd = root / directory
        expected = (out / 'main' / f'{name}.node.out').read_bytes()
        trials = {'main': [], 'head': []}
        for trial in range(5):
            for arm in (['main', 'head'] if trial % 2 == 0 else ['head', 'main']):
                _, env = environment(root, arm, name)
                if thp_off:
                    env['MIMALLOC_ALLOW_THP'] = '0'
                binary = out / arm / name
                with binary.open('rb') as executable:
                    while executable.read(1024 * 1024):
                        pass
                prefix = folder / f'{name}.{trial}.{arm}'
                prefix.parent.mkdir(parents=True, exist_ok=True)
                counter, rss = Path(str(prefix) + '.stat'), Path(str(prefix) + '.rss')
                command = ['setarch', '-R', str(binary), *args]
                actual = run(['setarch', '-R', 'perf', 'stat', '-x', ';', '-e', 'instructions:u',
                              '-o', str(counter), '/usr/bin/time', '-f', '%M', '-o', str(rss),
                              str(binary), *args], cwd, env, prefix, thp_off=thp_off)
                if actual != expected:
                    raise RuntimeError(f'{prefix}: sampled output differs')
                instructions = next(int(line.split(';')[0]) for line in counter.read_text().splitlines()
                                    if ';instructions:u;' in line)
                row = {'instructions': instructions, 'rss_kib': int(rss.read_text())}
                row.update(gc_counts(command, cwd, env, Path(str(prefix) + '.gc'), expected, thp_off))
                trials[arm].append(row)
                print(f'{name}/{trial}/{arm}: {row}', flush=True)
        medians = {arm: {key: statistics.median(r[key] for r in rows if r[key] is not None)
                         if any(r[key] is not None for r in rows) else None
                         for key in rows[0]} for arm, rows in trials.items()}
        ranges = {arm: {key: max(r[key] for r in rows) - min(r[key] for r in rows)
                        for key in ['instructions', 'rss_kib']} for arm, rows in trials.items()}
        results[name] = {'trials': trials, 'medians': medians, 'ranges': ranges}
        result_file.write_text(json.dumps(results, indent=2) + '\n')
        print(name, medians, flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--programs', default=','.join(['zod5k', *(k for k in PROGRAMS if k not in {'shapes-options', 'zod5k'})]))
    parser.add_argument('--thp-off', action='store_true')
    parser.add_argument('--tag', default='', help='separate diagnostic control results')
    parser.add_argument('action', choices=['compile-main', 'compile-head', 'measure'])
    args = parser.parse_args()
    root = args.hostdir.resolve()
    (root / 'tmp').mkdir(exist_ok=True)
    out = root / 'ab' / args.label
    names = args.programs.split(',')
    if args.action.startswith('compile-'):
        compile_arm(root, out, args.action.removeprefix('compile-'), names)
    else:
        measure(root, out, names, args.thp_off, args.tag)


if __name__ == '__main__':
    main()
