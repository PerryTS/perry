#!/usr/bin/env python3
"""Verify recorded profiles, candidates, correctness and performance evidence."""
from pathlib import Path
import hashlib
import json

root = Path(__file__).resolve().parent
repo = root.parents[3]
read = lambda p: json.loads((root / p).read_text())
rows = lambda p: [json.loads(s) for s in (root / p).read_text().splitlines() if s.startswith('{')]
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
q = read('qualification.json')
assert not q['all_row_parity']
reference = read('reference/provenance.json')
assert len(reference['source_sha256']) == 90
for arm, count, tests in [('string10', 90, 3278), ('string11', 91, 3279)]:
    p = read(arm + '/provenance.json')
    assert len(p['source_sha256']) == count
    assert p['source_sha256'] == read(arm + '/source-stamp.json')
    assert p['settings'] == reference['settings']
    assert p['effective_manifest_profiles'] == reference['effective_manifest_profiles']
    assert p['release_matches_dist']
    for worker in ['worker', 'lifetime-worker']:
        assert p['workers'][worker]['object_sha256'] == reference['workers'][worker]['object_sha256']
    assert f'test result: ok. {tests} passed; 0 failed; 4 ignored;' in (root / arm / 'tests.log').read_text()
    checks = read(arm + '/acceptance.json')
    assert len(checks) == 40 and all(r['matches_node'] for r in checks)
    for file, n in [('all-gc-stress.json', 38), ('forced-tape-validation.json', 3), ('no-json-growth-validation.json', 3)]:
        checks = read(arm + '/' + file)
        assert len(checks) == n and all(r['matches_node'] and r['live_subject'] for r in checks)
    checks = read(arm + '/extended-validation.json')
    assert len(checks) == 3 and all(r['matches_reference'] and r['known_preexisting_mismatch_only'] and r['live_subject'] for r in checks)
    for file in ['lifetime-fixture-validation.json', 'root-record-validation.json']:
        checks = read(arm + '/' + file)
        assert len(checks) == 9 and all(r['matches_node'] and r.get('live_subject', True) for r in checks)
    path = arm + '/performance/results/'
    checks = rows(path + 'recheck/verify.jsonl')
    assert len(checks) == 152 and all(r['correct'] for r in checks)
    for file, n in [('recheck/timing.jsonl', 760), ('recheck/memory.jsonl', 432)]:
        checks = rows(path + file)
        assert len(checks) == n and all(r['exit_code'] == 0 and 'error' not in r and r['tape'] is None for r in checks)
    checks = rows(path + 'lifetimes/raw.jsonl')
    assert len(checks) == 108 and all(not r['trace'] for r in checks)
    assert all(r['output_verified'] for r in checks if r['mode'] in ['latest', 'retain'])
    window = read(path + 'window-custom.json')
    assert window['quiet_gate_passed'] and window['load_before'][0] <= 2.5 and window['load_after'][0] <= 2.5
    observations = read(path + 'trace-observations.json')
    assert len(observations) == q[arm]['observations']
    assert all(r['monitor_exit'] == 0 and not r['external'] for r in observations)
    measured = read(path + 'recheck/metadata.json')['workers']
    assert measured['candidate'] == p['workers']['worker']['sha256']
    assert measured['checkpoint'] == reference['workers']['worker']['sha256']
checks = read('profiles/summary.json')
assert len(checks) == 4
for r in checks:
    assert r['sample_exit'] == 0 and r['worker_terminated_after_sample'] == -15
    assert r['trace_cycles'] == (40 if r['fixture'] == 'unicode_1m' else 1)
    assert r['trace_cycles'] == r['trace_full']
for folder, trials in [('scanner', 60), ('scanner2', 60), ('fused', 180), ('fused2', 210)]:
    checks = read(folder + '/results/raw.json')
    assert len(checks) == trials
    for fixture in {r['fixture'] for r in checks}:
        assert len({r['checksum'] for r in checks if r['fixture'] == fixture}) == 1
    for file, h in read(folder + '/metadata.json')['source_sha256'].items():
        assert sha(root / folder / file) == h
# Repeated and controlled windows are distinct from the original four-engine run.
for folder, observations in [('followup', 32), ('canonical', 31), ('fixed', 58)]:
    path = 'string11/' + folder + '/results/'
    window = read(path + 'window-custom.json')
    assert window['quiet_gate_passed'] and max(window['load_before'][0], window['load_after'][0]) <= 2.5
    checks = read(path + 'trace-observations.json')
    assert len(checks) == observations and all(r['monitor_exit'] == 0 and not r['external'] for r in checks)
checks = rows('string11/followup/results/focused/timing.jsonl')
assert len(checks) == 140 and all(r['exit_code'] == 0 and 'error' not in r for r in checks)
checks = rows('string11/followup/results/lifetimes/raw.jsonl')
assert len(checks) == 40 and all(not r['trace'] for r in checks)
assert all(r['output_verified'] for r in checks if r['mode'] in ['latest', 'retain'])
checks = read('string11/canonical/results/raw.json')
assert len(checks) == 32
assert len({r['executable_path'] for r in checks if r['phase'] == 'canonical'}) == 1
assert len({r['checksum'] for r in checks}) == 1
assert read('string11/canonical/results/metadata.json')['workers']['checkpoint'] == reference['workers']['lifetime-worker']['sha256']
traces = read('string11/canonical/results/trace-summary.json')
assert len(traces) == 4 and all(r['cycles'] == 37 and r['full'] == 28 for r in traces)
assert traces[0]['pointer_slots'] == traces[1]['pointer_slots'] == 36210217
checks = rows('string11/fixed/results/recheck/verify.jsonl')
assert len(checks) == 152 and all(r['correct'] for r in checks)
fixed = []
for file, n in [('recheck/timing.jsonl', 380), ('recheck/memory.jsonl', 216)]:
    checks = rows('string11/fixed/results/' + file)
    assert len(checks) == n and all(r['exit_code'] == 0 and 'error' not in r and r['tape'] is None for r in checks)
    fixed.extend(checks)
checks = rows('string11/fixed/results/lifetimes/raw.jsonl')
assert len(checks) == 108 and all(not r['trace'] for r in checks)
assert all(r['output_verified'] for r in checks if r['mode'] in ['latest', 'retain'])
fixed.extend(checks)
assert len({r['executable_path'] for r in fixed}) == 1
summary = read('string11/fixed/results/recheck/summary.json')
assert len(summary) == 38
unicode = next(r for r in summary if r['fixture'] == 'unicode_1m' and r['operation'] == 'parse')
assert -7.84 < unicode['cpu_us']['delta_percent'] < -7.81
assert not q['no_regression_acceptance'] and q['experimental']
stamp = read(('string11' if q['runtime_changes_retained'] else 'reference') + '/source-stamp.json')
assert all(sha(repo / f) == h for f, h in stamp.items())
print('Both release candidates, source provenance, correctness, all 38 rows and diagnostic evidence verified.')
