#!/usr/bin/env python3
"""Verify the reverted JSON construction candidate and qualified comparisons."""
from pathlib import Path
import hashlib
import json
import statistics

root = Path(__file__).resolve().parent
repo = root.parents[3]


def read(path):
    return json.loads((root / path).read_text())


def rows(path):
    return [json.loads(line) for line in (root / path).read_text().splitlines()
            if line.startswith('{')]


def quiet(path, count):
    window = read(path + '/window-custom.json')
    assert window['quiet_gate_passed']
    assert window['load_before'][0] <= 2.5 and window['load_after'][0] <= 2.5
    observations = read(path + '/trace-observations.json')
    assert len(observations) == count
    assert all(r['monitor_exit'] == 0 and not r['external'] for r in observations)


q = read('qualification.json')
assert not q['runtime_changes_retained'] and not q['no_regression_acceptance']
assert not q['all_row_parity'] and not q['initial_followup_qualified']
stamp = read('../gc-deferral/source-stamp.json')
assert len(stamp) == 90
assert all(hashlib.sha256((repo / f).read_bytes()).hexdigest() == sha for f, sha in stamp.items())
provenance = read('provenance.json')
assert provenance['source_sha256'] == read('source-stamp.json')
assert provenance['application_object_sha256'] == '28a4483ebffc124debdb5e223f4972cf9cdb27bdae59476981539cd7a27b5618'
assert '3278 passed; 0 failed; 4 ignored' in (root / 'tests.log').read_text()
assert len(read('acceptance.json')) == 40
assert all(r['matches_node'] for r in read('acceptance.json'))
for file, n in [('all-gc-stress.json', 38), ('forced-tape-validation.json', 3),
                ('no-json-growth-validation.json', 3)]:
    checks = read(file)
    assert len(checks) == n and all(r['matches_node'] and r['live_subject'] for r in checks)
checks = read('extended-validation.json')
assert len(checks) == 3 and all(r['matches_reference'] and r['known_preexisting_mismatch_only']
                               and r['live_subject'] for r in checks)
for file in ['lifetime-fixture-validation.json', 'root-record-validation.json']:
    checks = read(file)
    assert len(checks) == 9 and all(r['matches_node'] and r.get('live_subject', True) for r in checks)
assert {r['check']: r['exit_code'] for r in read('lints.json')} == {
    'node-version': 0, 'root-holders': 0, 'addr-class': 1, 'file-sizes': 1}
quiet('measurements', 64)
quiet('followup/results', 25)
checks = rows('measurements/recheck/verify.jsonl')
assert len(checks) == 190 and all(r['correct'] for r in checks)
for file, n in [('measurements/recheck/timing.jsonl', 570),
                ('measurements/recheck/memory.jsonl', 324),
                ('followup/results/focused/timing.jsonl', 224)]:
    checks = rows(file)
    assert len(checks) == n and all(r['exit_code'] == 0 and 'error' not in r for r in checks)
for file, n in [('measurements/lifetimes/raw.jsonl', 108),
                ('followup/results/lifetimes/raw.jsonl', 18)]:
    checks = rows(file)
    assert len(checks) == n and all(not r['trace'] for r in checks)
    assert all(r['output_verified'] for r in checks if r['mode'] in ['retain', 'latest'])
focused = rows('followup/results/focused/timing.jsonl')
paired = []
for rep in range(7):
    values = {engine: [r['cpu_ms'] for r in focused if r['fixture'] == 'numbers_1m'
                      and r['rep'] == rep and r['engine'] == engine]
              for engine in ['checkpoint', 'candidate']}
    assert all(len(v) == 2 for v in values.values())
    paired.append((statistics.mean(values['candidate']) / statistics.mean(values['checkpoint']) - 1) * 100)
assert all(1.48 < change < 1.73 for change in paired)
assert not read('followup-unqualified/qualification.json')['qualified']
assert not read('followup-unqualified/results/window-custom.json')['quiet_gate_passed']
for arm, dirty, slots in [('checkpoint', 2_344_560, 32_912_743),
                          ('candidate', 870_000, 29_963_623)]:
    trace = rows(f'diagnostics/{arm}.stderr')
    assert len(trace) == 25 and sum(r['collection_kind'] == 'minor' for r in trace) == 6
    assert sum(r['remembered_set']['dirty_slots_scanned'] for r in trace) == dirty
    assert sum(r['layout_scans']['pointer_slots_read'] for r in trace) == slots
    assert sum(r['copying_nursery']['promoted_objects'] for r in trace) == 3_485_192
print('Restored runtime, validation, qualified measurements and confirmed numeric regression verified.')
