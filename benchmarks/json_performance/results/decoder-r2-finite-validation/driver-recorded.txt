#!/usr/bin/env python3
"""Replay compiled escaped records with finite checksums; no build or timing claim.

Requires staged worker binaries. The original full build/GC validation command
is preserved verbatim in validate-recorded.txt, with its original output files.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[4]
BENCH = ROOT / 'benchmarks/json_performance'


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', type=Path, default=BENCH / '.work/decoder-r2/worker')
    parser.add_argument('--baseline-worker', type=Path, default=BENCH / '.work/main-e722/worker')
    parser.add_argument('--node', default=os.environ.get('NODE', 'node'))
    parser.add_argument('--results-dir', type=Path, required=True)
    args = parser.parse_args()
    worker = args.worker.resolve()
    baseline = args.baseline_worker.resolve()
    node_version = subprocess.check_output([args.node, '--version'], text=True).strip()
    assert node_version.removeprefix('v') == (ROOT / '.node-version').read_text().strip()
    cases_path = ROOT / 'crates/perry-runtime/src/json_tape/string_decode_tests.rs'
    text = cases_path.read_text().split('unsafe fn render', 1)[0]
    tokens = re.findall(r'r#"(.*?)"#', text, re.S)
    assert len(tokens) == 14
    metadata = dict(worker_sha256=sha(worker), baseline_worker_sha256=sha(baseline),
                    node_version=node_version, cases_sha256=sha(cases_path),
                    worker_js_sha256=sha(BENCH / 'worker.js'), driver_sha256=sha(__file__),
                    source_commit=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                    purpose='finite-checksum correctness replay; not performance evidence')
    out = args.results_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    clean = {k: v for k, v in os.environ.items() if not k.startswith('PERRY_')}
    stress = dict(PERRY_GC_SCHEDULE_SEED='10022', PERRY_GC_SCHEDULE_RATE='0.1',
                  PERRY_GC_SCHEDULE_ALLOC_KB='0', PERRY_GC_PROTECT_FROMSPACE='1')

    def run(label, command, fixture, mode, env=None):
        result = subprocess.run([*map(str, command), str(fixture), mode, '1', '0', 'verify'],
                                env=clean | (env or {}), capture_output=True, check=True, timeout=90)
        (out / (label + '.stdout')).write_bytes(result.stdout)
        (out / (label + '.stderr')).write_bytes(result.stderr)
        fields = list(map(float, re.search(rb'^RESULT (.+)$', result.stdout, re.M).group(1).split()))
        assert len(fields) == 7 and all(math.isfinite(n) for n in fields), label
        assert fields[-2:] == ([128, 0] if mode == 'scan' else [2, 0]), (label, fields)
        verify = [line for line in result.stdout.splitlines() if line.startswith((b'VERIFY ', b'KEEP '))]
        assert len(verify) == 2, label
        return verify

    matrix = []
    for case, record in enumerate(tokens[::2]):
        # Preserve duplicate and escaped keys verbatim; only add the scan field.
        if '"id":' not in record:
            record = '{"id":1,' + record[1:]
        fixture = out / ('escaped-record-' + str(case) + '.json')
        fixture.write_text('[' + ','.join([record] * 128) + ']')
        for mode in ['scan', 'sparse']:
            label = f'{case}-{mode}'
            expected = run('node-' + label, [args.node, BENCH / 'worker.js'], fixture, mode)
            prior = run('before-' + label, [baseline], fixture, mode)
            matches = {}
            for name, env in [('normal', {}), ('scheduled', stress), ('fullgc', {'PERRY_GEN_GC': '0'})]:
                actual = run('fixed-' + label + '-' + name, [worker], fixture, mode, env)
                assert actual == expected, (label, name)
                matches[name] = True
            matrix.append(dict(case=case, mode=mode, old_matches_node=prior == expected,
                               fixed_matches_node=matches, finite_checksums=True))
    assert sum(not row['old_matches_node'] for row in matrix) == 12
    (out / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    (out / 'matrix.json').write_text(json.dumps(matrix, indent=2) + '\n')
    print('PASS: 14 cases, 42 candidate comparisons; finite checksums in all 70 processes; 12 old failures')


if __name__ == '__main__':
    main()
