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
import shlex
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
               PERRY_RUN_TIMEOUT='30',
               RAYON_NUM_THREADS=os.environ.get('PERRY_VERIFY_RAYON_THREADS', '8'))
    original = (source/'run_parity_tests.sh').read_text()
    switch = 'elif [[ -n "${PERRY_NO_AUTO_OPTIMIZE:-}" && "$TEST_SUITE" == "all" ]] &&'
    assert original.count(switch) == 1
    runner = source/f'.buffer-b4-parity-{os.getpid()}.sh'
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
    keywords = args.keywords.split(',')
    selected = sorted(p for p in (source/'test-files').glob('test_gap_*')
                      if p.suffix in ['.ts', '.cts', '.mts']
                      and (any(k in p.stem for k in keywords) or '12094' in p.stem))
    selection = folder/'selection.txt'
    selection.write_text(''.join(str(p)+'\n' for p in selected))
    # Run the union once; overlapping keyword sets must not rerun fixtures.
    # Keep the canonical selection, skip list, normalizer and status handling.
    marker = 'declare -a SELECTED_FILES=()'
    text = runner.read_text()
    assert text.count(marker) == 1
    text = text.replace(marker, "TEST_FILES=()\nwhile IFS= read -r test_file; do\n"
                        "    TEST_FILES+=(\"$test_file\")\ndone < " + shlex.quote(str(selection))
                        + "\n" + marker)
    runner.write_text(text)
    try:
        journal = folder/'subset.jsonl'
        with (folder/'subset.log').open('w') as output:
            proc = subprocess.run(['bash', str(runner), '--filter', 'test_gap_',
                                   '--journal', str(journal)], cwd=source, env=env,
                                  stdout=output, stderr=subprocess.STDOUT)
        if journal.exists():
            for line in journal.read_text().splitlines():
                row = json.loads(line)
                if 'status' in row and 'id' in row: results[row['id']] = row['status']
        print(f'{args.arm}/subset: runner exit {proc.returncode}; {len(results)} of {len(selected)} results', flush=True)
    finally:
        runner.unlink(missing_ok=True)
    (folder/'results.json').write_text(json.dumps(results, indent=2)+'\n')
    if not results:
        raise SystemExit('No tests ran; inspect setup logs')

if __name__ == '__main__': main()
