#!/usr/bin/env python3
"""Use the canonical parity normalizer with one coherent, forced wrapper set.

The stock mixed-suite runner switches wrapper cases to auto-optimize. B2c/B4
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
from buffer_b4_validate import environment

KEYWORDS = ['buffer', 'typed', 'dataview', 'arraybuffer', 'zlib', 'crypto', 'tls', 'net', 'http', 'ws']

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--arm', choices=['main', 'head'], required=True)
    parser.add_argument('--keywords', default=','.join(KEYWORDS))
    args = parser.parse_args()
    source, target, env = environment(args.hostdir, args.arm)
    env.update(PERRY_BIN=str(target/'release/perry'), PERRY_FORCE_WELL_KNOWN='http,net,ws,zlib',
               PERRY_RUN_TIMEOUT='30', RAYON_NUM_THREADS='8')
    original = (source/'run_parity_tests.sh').read_text()
    switch = 'elif [[ -n "${PERRY_NO_AUTO_OPTIMIZE:-}" && "$TEST_SUITE" == "all" ]] &&'
    assert original.count(switch) == 1
    runner = source/'.buffer-b4-parity.sh'
    # The shared host forbids pkill. Enumerate only executables produced in
    # this runner's unique scratch directory, then stop them by their PID.
    import re
    original, replaced = re.subn(
        r'    if \[\[ "\$HOST_PLATFORM" != "windows" \]\] && command -v pkill.*?\n    fi',
        '    reap_lane_children', original, flags=re.S)
    assert replaced == 2
    cleanup = """
reap_lane_children() {
    python3 - "$PARITY_TMP" <<'PY_REAP'
import os, pathlib, signal, sys
prefix = sys.argv[1] + '/perry_parity_'
for entry in pathlib.Path('/proc').iterdir():
    if not entry.name.isdigit(): continue
    try:
        if os.readlink(entry/'exe').startswith(prefix):
            os.kill(int(entry.name), signal.SIGKILL)
    except (OSError, ProcessLookupError): pass
PY_REAP
}
"""
    runner.write_text(original.replace(switch, 'elif false &&').replace('cleanup_parity_run() {', cleanup+'\ncleanup_parity_run() {'))
    results = {}
    folder = args.hostdir/'gap'/args.arm
    folder.mkdir(parents=True, exist_ok=True)
    try:
        for keyword in args.keywords.split(','):
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
