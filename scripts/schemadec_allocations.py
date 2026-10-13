#!/usr/bin/env python3
"""Count arena allocation requests using the sampler in a GDB diagnostic run.

After gc_init, set the sampler interval below the minimum padded cell size.
Every runtime request records one sample, and every inline birth takes its
existing shared slow allocator. Refused no-collection requests and their
rooted retries are both counted. Promotion/copy allocations bypass the sampler.
This changes only the diagnostic process; no binary or source is patched.
The reported est_bytes is not meaningful at this interval: use samples only.
"""
import argparse
import ctypes
import json
from pathlib import Path
import re
import subprocess

from perf_schemadec_lane import environment


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--hostdir', type=Path, required=True)
    parser.add_argument('--arms', default='main,head')
    parser.add_argument('--parse-only', action='store_true')
    args = parser.parse_args()
    root = args.hostdir.resolve()
    if ctypes.CDLL(None, use_errno=True).prctl(41, 1, 0, 0, 0):
        raise OSError(ctypes.get_errno(), 'PR_SET_THP_DISABLE')
    results = {}
    for arm in args.arms.split(','):
        out = root / 'profiles' / arm
        counts = {}
        for n in (2, 6):
            log = out / f'N{n}.allocation-count.log'
            if not args.parse_only:
                commands = ['set pagination off', 'set confirm off',
                            'set debuginfod enabled off', 'set disable-randomization on',
                            'set language c', "break 'perry_runtime::gc::gc_init'",
                            'run', 'finish',
                            "set {unsigned long}&'perry_runtime::arena::alloc_sample::INTERVAL' = 2",
                            'continue']
                command = ['gdb', '-nx', '-batch']
                for entry in commands:
                    command += ['-ex', entry]
                command += ['--args', str(out / 'decode'), str(n)]
                with log.open('wb') as stream:
                    subprocess.run(command, cwd=root / 'programs/effect',
                                   env=dict(environment(root, arm), PERRY_ALLOC_SITE_SAMPLE='2'),
                                   stdout=stream, stderr=stream, check=True, timeout=300)
            text = log.read_text()
            assert re.search(r'\[Inferior .* exited normally\]', text), log
            match = re.search(r'\[alloc-site\] exit: interval=2 samples=(\d+).*inline_trips=(\d+)', text)
            assert match and match[2] == '0', 'all inline births must enter the shared allocator'
            counts[n] = int(match[1])
        results[arm] = {'counts': counts, 'arena_allocation_requests_per_decode': (counts[6] - counts[2]) / 4}
        print(arm, results[arm], flush=True)
    (root / 'measure/arena-request-counts.json').write_text(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
