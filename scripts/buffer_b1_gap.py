#!/usr/bin/env python3
"""Use the canonical parity normalizer with one coherent, forced wrapper set.

The stock mixed-suite runner switches wrapper cases to auto-optimize. B1
instead measures the same full prebuilt package/feature set on both arms,
forcing the archives that satisfy its three pump references on every link.
Only that switch is suppressed in a temporary copy of the canonical runner.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from buffer_b1_validate import environment

KEYWORDS = ['buffer', 'typed', 'dataview', 'arraybuffer', 'zlib', 'crypto', 'tls']

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--arm', choices=['main', 'head'], required=True)
    args = parser.parse_args()
    source, target, env = environment(args.hostdir, args.arm)
    env.update(PERRY_BIN=str(target/'release/perry'), PERRY_FORCE_WELL_KNOWN='http,net,ws,zlib',
               PERRY_RUN_TIMEOUT='30', RAYON_NUM_THREADS='8')
    original = (source/'run_parity_tests.sh').read_text()
    switch = 'elif [[ -n "${PERRY_NO_AUTO_OPTIMIZE:-}" && "$TEST_SUITE" == "all" ]] &&'
    assert original.count(switch) == 1
    runner = source/'.buffer-b1-parity.sh'
    runner.write_text(original.replace(switch, 'elif false &&'))
    results = {}
    folder = args.hostdir/'gap'/args.arm
    folder.mkdir(parents=True, exist_ok=True)
    try:
        for keyword in KEYWORDS:
            journal = folder/f'{keyword}.jsonl'
            with (folder/f'{keyword}.log').open('w') as output:
                proc = subprocess.run(['bash', str(runner), '--filter', 'test_gap_', '--filter', keyword,
                                       '--journal', str(journal)], cwd=source, env=env, stdout=output,
                                      stderr=subprocess.STDOUT)
            if journal.exists():
                for line in journal.read_text().splitlines():
                    row = json.loads(line)
                    if 'status' in row and 'id' in row: results[row['id']] = row['status']
            print(f'{args.arm}/{keyword}: runner exit {proc.returncode}; {len(results)} unique results', flush=True)
    finally:
        runner.unlink(missing_ok=True)
    (folder/'results.json').write_text(json.dumps(results, indent=2)+'\n')
    if not results:
        raise SystemExit('No tests ran; inspect setup logs')

if __name__ == '__main__': main()
