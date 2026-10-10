#!/usr/bin/env python3
"""Readpath3 witness gate: pinned Linux lane, five base-subtracted trials."""
import argparse
import json
import os
import re
from pathlib import Path
import statistics
import subprocess


def checked(cmd, env, log):
    p = subprocess.run(cmd, env=env, capture_output=True, timeout=600)
    Path(str(log) + '.out').write_bytes(p.stdout)
    Path(str(log) + '.err').write_bytes(p.stderr)
    if p.returncode:
        raise RuntimeError(f'{cmd}: exit {p.returncode}\n{p.stderr.decode()[-2000:]}')
    return p.stdout


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--hostdir', type=Path, required=True)
    p.add_argument('--label', required=True)
    p.add_argument('--arm', choices=['head', 'main'], default='head')
    p.add_argument('--names', default='m7,m7s,m7s-computed,wm-get,wm-has,wm-set,wm-direct-get,wm-direct-has,wm-direct-set,inh,abs,inh2,abs2,inh3,abs3,s3,s2,own,full,a2,a2-computed')
    args = p.parse_args()
    root = args.hostdir
    lane = root / 'main-arm' if args.arm == 'main' else root
    prebuilt = root / ('prebuilt-' + args.arm)
    for crate in ('perry-runtime', 'perry-ui-geisterhand'):
        (prebuilt / 'crates' / crate).mkdir(parents=True, exist_ok=True)
    out = root / 'witness' / args.label
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_NO_CACHE='1', CARGO_BUILD_JOBS='8', RAYON_NUM_THREADS='8', TMPDIR=str(root / 'tmp'),
               CARGO_TARGET_DIR=str(lane / 'target'), PERRY_RUNTIME_DIR=str(lane / 'target/release'),
               PERRY_WORKSPACE_ROOT=str(prebuilt), PERRY_SKIP_BUILD='1')
    (root / 'tmp').mkdir(exist_ok=True)
    references = ['base', 'wm-base']
    if any(name.startswith('wm-direct-') for name in args.names.split(',')):
        references.append('wm-direct-base')
    names = list(dict.fromkeys([*references, *args.names.split(',')]))
    for name in names:
        source = root / 'src/benchmarks/readpath3' / f'{name}.ts'
        checked([str(lane / 'target/release/perry'), 'compile', '--no-auto-optimize', str(source), '-o', str(out / name)], env, out / f'{name}.compile')
        expected = checked(['node', '--experimental-strip-types', str(source)], env, out / f'{name}.node')
        actual = checked([str(out / name)], dict(env, PERRY_METHOD_SITE_STATS='1'), out / f'{name}.stats')
        if expected != actual:
            raise RuntimeError(f'{name}: node differential failed')
        print(f'{name}: parity PASS', flush=True)
    if args.label.startswith('C0'):
        for name in ('wm-get', 'wm-has', 'wm-set'):
            if name not in names:
                continue
            stats = (out / f'{name}.stats.err').read_text()
            match = re.search(r'refused\.inh_proto_not_in_shape=(\d+)', stats)
            if match is None or int(match.group(1)) < 1000000:
                raise RuntimeError(f'{name}: opaque CLASS refusal witness was not live')
    counts = {name: [] for name in names}
    for trial in range(5):
        for name in (names if trial % 2 == 0 else names[::-1]):
            stat = out / f'{name}.{trial}.stat'
            actual = checked(['setarch', '-R', 'perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', str(stat), str(out / name)], env, out / f'{name}.{trial}')
            if actual != (out / f'{name}.node.out').read_bytes():
                raise RuntimeError(f'{name}/{trial}: diagnostics-OFF output differs from Node')
            counts[name].append(next(int(line.split(';')[0]) for line in stat.read_text().splitlines() if ';instructions:u;' in line))
    medians = {name: statistics.median(v) for name, v in counts.items()}
    rows = {}
    for name in args.names.split(','):
        reference = ('wm-direct-base' if name.startswith('wm-direct-') else
                     'wm-base' if name.startswith('wm-') else 'base')
        rows[name] = {'instructions_per_op': (medians[name] - medians[reference]) / 1e6,
                      'base': reference, 'median': medians[name],
                      'range': max(counts[name]) - min(counts[name])}
    gate = None
    if args.label == 'C0' and 'm7s' in names:
        stats = (out / 'm7s.stats.err').read_text()
        match = re.search(r'refused\.inh_hop_refused=(\d+)', stats)
        actual = int(match.group(1)) if match else 0
        gate = {'required_m7s_refusals': 1000000, 'actual_m7s_refusals': actual, 'pass': actual == 1000000}
    (out / 'results.json').write_text(json.dumps({'counts': counts, 'rows': rows, 'design_gate': gate}, indent=2))
    for name, row in rows.items():
        print(name, row, flush=True)
    if gate is not None and not gate['pass']:
        raise SystemExit(f'C0 design gate failed: {gate}')


if __name__ == '__main__':
    main()
