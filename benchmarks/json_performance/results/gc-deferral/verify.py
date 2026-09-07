#!/usr/bin/env python3
"""Validate the recorded source, trial counts, correctness and quiet gates."""
from pathlib import Path
import hashlib
import json

root = Path(__file__).resolve().parent
repo = root.parents[3]


def read(path):
    return json.loads((root / path).read_text())


def lines(path):
    return [json.loads(line) for line in (root / path).read_text().splitlines()]


stamp = read('source-stamp.json')
assert len(stamp) == 90
assert all(hashlib.sha256((repo / file).read_bytes()).hexdigest() == sha
           for file, sha in stamp.items())
assert '3278 passed; 0 failed; 4 ignored' in (root / 'runtime-tests.log').read_text()
compiled = read('compiled-validation.json')
assert len(compiled) == 40 and all(row['matches_node'] for row in compiled)
stress = read('gc-stress-validation.json')
assert len(stress) == 38 and all(row['matches_node'] and row['live_subject'] for row in stress)
for file in ['forced-tape-validation.json', 'no-json-growth-validation.json']:
    rows = read(file)
    assert len(rows) == 3 and all(row['matches_node'] and row['live_subject'] for row in rows)
extended = read('extended-validation.json')
assert len(extended) == 3 and all(row['matches_reference']
                               and row['known_preexisting_mismatch_only']
                               and row['live_subject'] for row in extended)
lifetime_fixture = read('lifetime-fixture-validation.json')
assert len(lifetime_fixture) == 9
assert all(row['matches_node'] and row.get('live_subject', True) for row in lifetime_fixture)
assert sum('PERRY_GC_SCHEDULE_SEED' in row['config'] for row in lifetime_fixture) == 4

verify = lines('verify.jsonl')
assert len(verify) == 152 and all(row['correct'] for row in verify)
for file, count in [('timing.jsonl', 456), ('memory.jsonl', 432)]:
    rows = lines(file)
    assert len(rows) == count and all('error' not in row and row['exit_code'] == 0 for row in rows)
assert read('window-fastpaths.json')['quiet_gate_passed']
assert read('paired/window.json')['quiet_gate_passed']
for file in ['external-observations.json', 'paired/observations.json']:
    assert all(row['monitor_exit'] == 0 and not row['external'] for row in read(file))
pairs = list((root / 'paired').glob('*.jsonl'))
assert len(pairs) == 38
for file in pairs:
    rows = [json.loads(line) for line in file.read_text().splitlines()]
    assert len(rows) == 14
    assert all(row['exit_code'] == 0 and 'error' not in row for row in rows)
assert len(lines('retained-empty/raw.jsonl')) == 24

assert not read('lifetimes-unqualified/window.json')['quiet_gate_passed']
assert len(lines('lifetimes-unqualified/raw.jsonl')) == 252
assert read('lifetimes/window.json')['quiet_gate_passed']
assert len(lines('lifetimes/raw.jsonl')) == 252
assert len(lines('lifetimes/trace-summary.jsonl')) == 12
assert len(read('lifetimes/summary.json')) == 42
observations = read('lifetimes/observations.json')
assert len(observations) == 29
assert all(row['monitor_exit'] == 0 and not row['external'] for row in observations)

validation = read('validation.json')
assert validation['lifetime_final_qualified']
assert not validation['lifetime_retry_in_progress']
assert not validation['all_row_acceptance']
assert not validation['no_regression_acceptance']
assert not validation['complete_semantic_acceptance']
print('Source, runtime/compiled/moving checks, full matrix, paired and lifetime evidence verified.')
