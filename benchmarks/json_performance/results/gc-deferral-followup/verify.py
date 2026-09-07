#!/usr/bin/env python3
"""Verify the retained source and the disposition of both experiments."""
from pathlib import Path
import hashlib
import json

root = Path(__file__).resolve().parent
repo = root.parents[3]


def read(path):
    return json.loads((root / path).read_text())


def rows(path):
    return [json.loads(line) for line in (root / path).read_text().splitlines()]


stamp = read('../gc-deferral/source-stamp.json')
assert len(stamp) == 90
assert all(hashlib.sha256((repo / file).read_bytes()).hexdigest() == sha
           for file, sha in stamp.items())
fixture = repo / 'test-files/test_gap_json_root_record.ts'
fixture_sha = hashlib.sha256(fixture.read_bytes()).hexdigest()
retained = read('defer2-root-record-validation.json')
assert len(retained) == 9
assert all(row['matches_node'] and row.get('live_subject', True)
           and row['source_sha256'] == fixture_sha for row in retained)
moving = [r for r in retained if 'PERRY_GC_SCHEDULE_SEED' in r['config']]
assert len(moving) == 4
assert all(r['counters']['scheduled_collections'] == 70
           and r['counters']['copying_minors'] == 75
           and r['counters']['loop_polls'] == 64 for r in moving)

assert len(rows('diagnosis/raw.jsonl')) == 54
assert sum(r['trace'] for r in rows('diagnosis/raw.jsonl')) == 18
assert read('diagnosis/window.json')['quiet_gate_passed']
assert all(r['monitor_exit'] == 0 and not r['external']
           for r in read('diagnosis/observations.json'))
assert not read('discarded-root-emitter/qualification.json')['qualified']

candidate = 'discarded-stack-plan/'
assert read(candidate + 'qualification.json')['qualified']
assert not read(candidate + 'qualification.json')['runtime_change_retained']
assert '3278 passed; 0 failed; 4 ignored' in (root / candidate / 'runtime-tests.log').read_text()
core = read(candidate + 'acceptance.json')
assert len(core) == 40 and all(r['matches_node'] for r in core)
stress = read(candidate + 'all-gc-stress.json')
assert len(stress) == 38 and all(r['matches_node'] and r['live_subject'] for r in stress)
for file in ['forced-tape-validation.json', 'no-json-growth-validation.json']:
    checks = read(candidate + file)
    assert len(checks) == 3 and all(r['matches_node'] and r['live_subject'] for r in checks)
extended = read(candidate + 'extended-validation.json')
assert len(extended) == 3 and all(r['matches_reference']
                                and r['known_preexisting_mismatch_only']
                                and r['live_subject'] for r in extended)
for file in ['root-record-validation.json', 'lifetime-fixture-validation.json']:
    checks = read(candidate + file)
    assert len(checks) == 9 and all(r['matches_node'] and r.get('live_subject', True)
                                  for r in checks)

measurements = candidate + 'measurements/'
checks = rows(measurements + 'verify.jsonl')
assert len(checks) == 190 and all(r['correct'] for r in checks)
for file, count in [('timing.jsonl', 570), ('memory.jsonl', 324)]:
    checks = rows(measurements + file)
    assert len(checks) == count and all(r['exit_code'] == 0 and 'error' not in r for r in checks)
assert len(read(measurements + 'summary.json')) == 38
assert len(read(measurements + 'memory-summary.json')) == 36
for path, expected in [(measurements, 33), (candidate + 'lifetimes/', 27)]:
    window = read(path + 'window.json')
    assert window['quiet_gate_passed']
    assert window['load_before'][0] <= 2.5 and window['load_after'][0] <= 2.5
    observations = read(path + 'observations.json')
    assert len(observations) == expected
    assert all(r['monitor_exit'] == 0 and not r['external'] for r in observations)
lifetimes = rows(candidate + 'lifetimes/raw.jsonl')
assert len(lifetimes) == 108 and all(not r['trace'] for r in lifetimes)
assert all(r['output_verified'] for r in lifetimes if r['mode'] in ['latest', 'retain'])
assert len(read(candidate + 'lifetimes/summary.json')) == 12
validation = read('validation.json')
assert not validation['runtime_changes_retained']
assert not validation['all_row_parity'] and not validation['no_regression_acceptance']
print('Retained runtime, new fixture, rejected experiments and qualified evidence verified.')
