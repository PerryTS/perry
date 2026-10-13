#!/usr/bin/env python3
"""Scoped raw-output gap A/B for capture cells and generic property keys.

Run on the lane's allowed build CPUs with separate, already built arm targets.
Every process is a direct child with a timeout; no name-based process cleanup.
"""
import argparse
import json
from pathlib import Path
import re
import shlex
import subprocess

import perf_objops_lane as common

SELECT = re.compile(r'test_gap_.*(?:scope|closure|capture|sso|key|property|descriptor|spread|assign|field|tdz|lexical|equality|nan|iterator)')
EXCLUDE = re.compile(r'(?:worker|stream|fetch|http|socket|tls|crypto|websocket|async|child_process)')


def execute(command, cwd, env, prefix):
    try:
        result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, timeout=120)
        rc, stdout, stderr = result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired as error:
        rc, stdout, stderr = 124, error.stdout or b'', error.stderr or b''
    Path(str(prefix) + '.out').write_bytes(stdout)
    Path(str(prefix) + '.err').write_bytes(stderr)
    return rc, stdout


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    args = parser.parse_args()
    root = args.hostdir.resolve()
    destination = root / 'measure/gaps'
    destination.mkdir(parents=True, exist_ok=True)
    results = {}
    for source in sorted((root / 'src/test-files').glob('test_gap_*.*ts')):
        if not SELECT.search(source.name) or EXCLUDE.search(source.name):
            continue
        text = source.read_text()
        # Keep this a leaf-program suite; process and server lifecycle belong
        # to their purpose-built runners. Avoid package builds inside the gate.
        if re.search(r'child_process|worker_threads|WebAssembly|\b(?:from|require\()\s*[\"\'](?:node:)?(?:events|net|http|https|zlib|ws)', text):
            continue
        env = common.environment(root, 'head')
        expected_rc, expected = execute([*common.NODE, str(source)], root / 'src', env,
                                        destination / (source.stem + '.node'))
        row = {'node': expected_rc}
        if expected_rc != 0:
            results[source.name] = row
            continue
        extras = re.search(r'^\s*//\s*parity-env:\s*(.*)$', text, re.M)
        flags = dict(item.split('=', 1) for item in shlex.split(extras[1]) if '=' in item) if extras else {}
        for arm in ('main', 'head'):
            env = common.environment(root, arm)
            env.update(flags)
            prefix = destination / (source.stem + '.' + arm)
            binary = destination / (source.stem + '-' + arm)
            rc, _ = execute([str(Path(env['CARGO_TARGET_DIR']) / 'release/perry'),
                             'compile', str(source), '-o', str(binary)], root / 'src', env,
                            destination / (source.stem + '.' + arm + '-compile'))
            if rc:
                row[arm] = {'compile': rc}
            else:
                rc, stdout = execute([str(binary)], root / 'src', env, prefix)
                row[arm] = {'run': rc, 'matches_node': stdout == expected}
        results[source.name] = row
        (destination / 'results.json').write_text(json.dumps(results, indent=2))
        print(source.name, row, flush=True)
    (destination / 'results.json').write_text(json.dumps(results, indent=2))
    regressions = [name for name, row in results.items() if row.get('main', {}).get('matches_node')
                   and not row.get('head', {}).get('matches_node')]
    summary = {'selected': len(results), 'oracle_success': sum(r['node'] == 0 for r in results.values()),
               'main_pass': sum(r.get('main', {}).get('matches_node', False) for r in results.values()),
               'head_pass': sum(r.get('head', {}).get('matches_node', False) for r in results.values()),
               'regressions': regressions}
    (destination / 'summary.json').write_text(json.dumps(summary, indent=2))
    print(summary, flush=True)
    return bool(regressions)


if __name__ == '__main__':
    raise SystemExit(main())
