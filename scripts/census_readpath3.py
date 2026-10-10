#!/usr/bin/env python3
"""Run the opt-in C0 census with Node output checks; no builds or installs."""
import argparse
import json
import os
from pathlib import Path
import subprocess

PROGRAMS = {
    'property-prototype': ('programs', 'property-prototype.ts', []),
    'effect': ('programs', 'effect.ts', []),
    'schema-record': ('programs', 'schema-record.ts', []),
    'fastify': ('realprog/pk', 'fastify/inject.ts', ['5000', '300']),
    'qs-parse': ('realprog/pk', 'qs/parse_nested.ts', ['20000', '1000']),
    'qs-stringify': ('realprog/pk', 'qs/stringify_nested.ts', ['20000', '1000']),
    'buffer_heavy': ('drivers', 'buffer_heavy.ts', []),
    'worker_heavy': ('drivers', 'worker_heavy.ts', []),
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--label', default='C0')
    args = parser.parse_args()
    root = args.hostdir
    out = root / 'census' / args.label
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PERRY_NO_CACHE='1', PERRY_ALLOW_PERRY_FEATURES='1',
               CARGO_BUILD_JOBS='8', RAYON_NUM_THREADS='8', TMPDIR=str(root / 'tmp'))
    status = {}
    for name, (directory, source, argv) in PROGRAMS.items():
        cwd = root / directory
        binary = out / name
        try:
            commands = [
                ('compile', [str(root / 'target/release/perry'), 'compile', '--no-auto-optimize', source, '-o', str(binary)], env),
                ('node', ['node', '--experimental-strip-types', source, *argv], env),
                ('perry', [str(binary), *argv], dict(env, PERRY_METHOD_SITE_STATS='1')),
            ]
            results = {}
            for label, command, process_env in commands:
                p = subprocess.run(command, cwd=cwd, env=process_env, capture_output=True, timeout=900)
                (out / f'{name}.{label}.out').write_bytes(p.stdout)
                (out / f'{name}.{label}.err').write_bytes(p.stderr)
                if p.returncode:
                    raise RuntimeError(f'{label}: exit {p.returncode}: {p.stderr.decode(errors="replace")[-600:]}')
                results[label] = p.stdout
            status[name] = 'PASS' if results['perry'] == results['node'] else 'OUTPUT MISMATCH'
        except Exception as exc:
            status[name] = str(exc)
        (out / 'status.json').write_text(json.dumps(status, indent=2))
        print(name, status[name], flush=True)


if __name__ == '__main__':
    main()
