#!/usr/bin/env python3
"""Compile selected readpath3 gaps with the explicitly built runtime."""
import argparse
import json
import os
import shlex
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--arm', choices=['head', 'main'], default='head')
    parser.add_argument('--sources', nargs='+', required=True)
    args = parser.parse_args()
    root = args.hostdir
    lane = root / 'main-arm' if args.arm == 'main' else root
    prebuilt = root / ('prebuilt-' + args.arm)
    for crate in ('perry-runtime', 'perry-ui-geisterhand'):
        (prebuilt / 'crates' / crate).mkdir(parents=True, exist_ok=True)
    out = root / 'gaps' / args.label
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_NO_CACHE='1', PERRY_ALLOW_PERRY_FEATURES='1',
               CARGO_BUILD_JOBS='8', RAYON_NUM_THREADS='8', TMPDIR=str(root / 'tmp'),
               CARGO_TARGET_DIR=str(lane / 'target'), PERRY_RUNTIME_DIR=str(lane / 'target/release'),
               PERRY_WORKSPACE_ROOT=str(prebuilt), PERRY_SKIP_BUILD='1')
    status = {}
    for relative in args.sources:
        source = root / relative
        name = source.stem
        node_args = []
        for line in source.read_text().splitlines()[:20]:
            if line.startswith('// parity-node-argv:'):
                node_args = shlex.split(line.split(':', 1)[1])
        commands = [
            ('compile', [str(lane / 'target/release/perry'), 'compile', '--no-auto-optimize', str(source), '-o', str(out / name)]),
            ('node', ['node', '--experimental-strip-types', *node_args, str(source)]),
            ('perry', [str(out / name)]),
        ]
        outputs = {}
        for label, command in commands:
            process = subprocess.run(command, env=env, capture_output=True, timeout=600)
            (out / f'{name}.{label}.out').write_bytes(process.stdout)
            (out / f'{name}.{label}.err').write_bytes(process.stderr)
            if process.returncode:
                status[name] = f'{label}: exit {process.returncode}'
                break
            outputs[label] = process.stdout
        else:
            status[name] = 'PASS' if outputs['node'] == outputs['perry'] else 'OUTPUT MISMATCH'
        print(name, status[name], flush=True)
        (out / 'status.json').write_text(json.dumps(status, indent=2))
    if any(value != 'PASS' for value in status.values()):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
