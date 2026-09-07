#!/usr/bin/env python3
"""Check retained source and the qualified, rejected JSON experiments."""
from pathlib import Path
import hashlib
import json

root = Path(__file__).resolve().parent
repo = root.parents[3]


def read(path):
    return json.loads((root / path).read_text())


def rows(path):
    return [json.loads(line) for line in (root / path).read_text().splitlines()]


def quiet(path):
    w = read(path + '/window.json')
    assert w['quiet_gate_passed']
    assert w['load_before'][0] <= 2.5 and w['load_after'][0] <= 2.5
    observations = read(path + '/observations.json')
    assert observations and all(r['monitor_exit'] == 0 and not r['external']
                                for r in observations)


stamp = read('../gc-deferral/source-stamp.json')
assert len(stamp) == 90
assert all(hashlib.sha256((repo / f).read_bytes()).hexdigest() == sha
           for f, sha in stamp.items())
assert read('qualification.json')['retained_runtime'] == '0327b9460a749592deb354d72ebad29bf4ae4bba'
assert not read('qualification.json')['runtime_changes_retained']
assert not read('qualification.json')['no_regression_acceptance']
quiet('layout')
assert len(rows('layout/verify.jsonl')) == 40
assert all(r['correct'] for r in rows('layout/verify.jsonl'))
assert len(rows('layout/timing.jsonl')) == 224
assert all(r['exit_code'] == 0 and 'error' not in r for r in rows('layout/timing.jsonl'))
assert len(read('layout/summary.json')) == 8
assert read('patch-reconstruction.json')['verified']
for arm in ['admission', 'direct-entry']:
    assert hashlib.sha256((root / arm / 'candidate.patch').read_bytes()).hexdigest() == read('patch-reconstruction.json')['arms'][arm]['patch_sha256']
    assert '3278 passed; 0 failed; 4 ignored' in (root / arm / 'runtime-tests.log').read_text()
    provenance = read(arm + '/provenance.json')
    assert provenance['source_sha256'] == read(arm + '/source-stamp.json')
    assert provenance['application_object_sha256'] == '28a4483ebffc124debdb5e223f4972cf9cdb27bdae59476981539cd7a27b5618'
    assert len(read(arm + '/source-stamp.json')) == 90
    checks = read(arm + '/acceptance.json')
    assert len(checks) == 40 and all(r['matches_node'] for r in checks)
    checks = read(arm + '/all-gc-stress.json')
    assert len(checks) == 38 and all(r['matches_node'] and r['live_subject'] for r in checks)
    for file in ['forced-tape-validation.json', 'no-json-growth-validation.json']:
        checks = read(arm + '/' + file)
        assert len(checks) == 3 and all(r['matches_node'] and r['live_subject'] for r in checks)
    checks = read(arm + '/extended-validation.json')
    assert len(checks) == 3 and all(r['matches_reference'] and r['known_preexisting_mismatch_only']
                                   and r['live_subject'] for r in checks)
    for file in ['lifetime-fixture-validation.json', 'root-record-validation.json']:
        checks = read(arm + '/' + file)
        assert len(checks) == 9 and all(r['matches_node'] and r.get('live_subject', True) for r in checks)
    m = arm + '/measurements/'
    quiet(m.rstrip('/'))
    checks = rows(m + 'verify.jsonl')
    assert len(checks) == 190 and all(r['correct'] for r in checks)
    for file, n in [('timing.jsonl', 570), ('memory.jsonl', 324)]:
        checks = rows(m + file)
        assert len(checks) == n and all(r['exit_code'] == 0 and 'error' not in r for r in checks)
    assert len(read(m + 'summary.json')) == 38
    assert len(read(m + 'memory-summary.json')) == 36
    life = arm + '/lifetimes/'
    if arm == 'admission':
        quiet(life.rstrip('/'))
    else:
        assert not read(life + 'qualification.json')['qualified']
        assert not read(life + 'window.json')['quiet_gate_passed']
    checks = rows(life + 'raw.jsonl')
    assert len(checks) == 108 and all(not r['trace'] for r in checks)
    assert all(r['output_verified'] for r in checks if r['mode'] in ['retain', 'latest'])
    assert len(read(life + 'summary.json')) == 12
    lints = {r['check']: r['exit_code'] for r in read(arm + '/lints.json')}
    assert lints == {'node-version': 0, 'root-holders': 0, 'addr-class': 1, 'file-sizes': 1}
quiet('trace-lifetime')
checks = rows('trace-lifetime/raw.jsonl')
assert len(checks) == 6 and all(r['trace'] for r in checks)
for r in checks:
    events = rows(f"trace-lifetime/{r['engine']}-{r['rep']}.trace.jsonl")
    assert r['cycles'] == len(events)
    assert r['pause_us'] == sum(e['pause_us'] for e in events)
print('Retained source, both rejected candidates, layout control and lifetime evidence verified.')
