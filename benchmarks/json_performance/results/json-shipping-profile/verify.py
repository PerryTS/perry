#!/usr/bin/env python3
"""Verify the default-release comparison and the rejected candidate."""
from pathlib import Path
import hashlib
import json
import statistics
import tomllib

root = Path(__file__).resolve().parent
repo = root.parents[3]
read = lambda p: json.loads((root / p).read_text())
rows = lambda p: [json.loads(s) for s in (root / p).read_text().splitlines() if s.startswith('{')]
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()

q = read('qualification.json')
assert not q['runtime_changes_retained'] and not q['no_regression_acceptance']
assert not q['all_row_parity']
stamp = read('ship-checkpoint/source-stamp.json')
assert len(stamp) == 90 and all(sha(repo / f) == s for f, s in stamp.items())
reference = read('ship-checkpoint/provenance.json')
candidate = read('ship-prefix/provenance.json')
assert reference['settings'] == candidate['settings']
assert reference['effective_manifest_profiles'] == candidate['effective_manifest_profiles']
for arm, p in [('ship-checkpoint', reference), ('ship-prefix', candidate)]:
    assert p['source_sha256'] == read(arm + '/source-stamp.json')
    assert p['release_matches_dist']
    assert p['workspace_manifest_sha256'] == sha(root / 'workspace-manifest.toml')
    assert p['cargo_config_sha256'] == sha(root / 'cargo-config.toml')
    for name, profile in p['effective_manifest_profiles'].items():
        assert profile['codegen-units'] == (16 if name.endswith('-static') else 1)
    checks = read(arm + '/acceptance.json')
    assert len(checks) == 40 and all(r['matches_node'] for r in checks)
    for file, n in [('all-gc-stress.json', 38), ('forced-tape-validation.json', 3),
                    ('no-json-growth-validation.json', 3)]:
        checks = read(arm + '/' + file)
        assert len(checks) == n and all(r['matches_node'] and r['live_subject'] for r in checks)
    checks = read(arm + '/extended-validation.json')
    assert len(checks) == 3 and all(r['matches_reference'] and r['known_preexisting_mismatch_only']
                                   and r['live_subject'] for r in checks)
    for file in ['lifetime-fixture-validation.json', 'root-record-validation.json']:
        checks = read(arm + '/' + file)
        assert len(checks) == 9 and all(r['matches_node'] and r.get('live_subject', True) for r in checks)
for worker in ['worker', 'lifetime-worker']:
    assert reference['workers'][worker]['object_sha256'] == candidate['workers'][worker]['object_sha256']
    assert reference['workers'][worker]['sha256'] != candidate['workers'][worker]['sha256']
assert reference['archives']['libperry_runtime.a'] != candidate['archives']['libperry_runtime.a']
profiles = tomllib.loads((root / 'workspace-manifest.toml').read_text())['profile']
for name, actual in reference['effective_manifest_profiles'].items():
    release = {k: v for k, v in profiles['release'].items() if k != 'package'}
    dist = dict(release)
    release.update(profiles['release']['package'].get(name, {}))
    dist.update({k: v for k, v in profiles['dist'].items() if k not in ['package', 'inherits']})
    dist.update(profiles['dist']['package'].get(name, {}))
    assert release == dist == actual
for path, count in [('measurements', 83), ('followup/results', 15)]:
    window = read(path + '/window-custom.json')
    assert window['quiet_gate_passed'] and window['load_before'][0] <= 2.5 and window['load_after'][0] <= 2.5
    observations = read(path + '/trace-observations.json')
    assert len(observations) == count and all(r['monitor_exit'] == 0 and not r['external'] for r in observations)
checks = rows('measurements/recheck/verify.jsonl')
assert len(checks) == 190 and all(r['correct'] for r in checks)
for file, n in [('measurements/recheck/timing.jsonl', 950), ('measurements/recheck/memory.jsonl', 540),
                ('followup/results/focused/timing.jsonl', 112)]:
    checks = rows(file)
    assert len(checks) == n and all(r['exit_code'] == 0 and 'error' not in r for r in checks)
for file, n in [('measurements/lifetimes/raw.jsonl', 108), ('followup/results/lifetimes/raw.jsonl', 18)]:
    checks = rows(file)
    assert len(checks) == n and all(not r['trace'] for r in checks)
    assert all(r['output_verified'] for r in checks if r['mode'] in ['retain', 'latest'])
focused = rows('followup/results/focused/timing.jsonl')
for rep in range(7):
    values = {e: [r['cpu_ms'] for r in focused if r['fixture'] == 'records_object_1m'
                   and r['rep'] == rep and r['engine'] == e] for e in ['checkpoint', 'candidate']}
    assert all(len(v) == 2 for v in values.values())
    change = (statistics.mean(values['candidate']) / statistics.mean(values['checkpoint']) - 1) * 100
    assert 0.21 < change < 0.74
summary = read('measurements/recheck/summary.json')
assert len(summary) == 38
assert sum(r['cpu_us']['checkpoint']['median'] <= min(r['cpu_us']['node']['median'], r['cpu_us']['bun']['median']) for r in summary) == 15
assert (root / 'measurements/recheck/all-38.csv').exists()
print('Default-release profiles, both validated binaries, all 38 rows and confirmed regression verified.')
