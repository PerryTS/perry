#!/usr/bin/env python3
"""Verify source identity, correctness evidence and saved measurement counts."""
from pathlib import Path
import hashlib
import json
import statistics
import subprocess

root = Path(__file__).resolve().parent
repo = root.parents[3]
read = lambda name: json.loads((root / name).read_text())
rows = lambda name: [json.loads(line) for line in (root / name).read_text().splitlines()]
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()

q = read('qualification.json')
assert q['experimental'] and not q['no_regression_acceptance']
assert q['all_38_complete'] and not q['older_full_inventory_complete']
assert not q['gc_policy_changed']
for name, digest in read('manifest.json').items():
    assert sha(root / name) == digest, name

commits = read('source-commit.json')
for arm, commit in [('depth22', commits['depth22']), ('depth23', commits['depth23'])]:
    stamp = read(arm + '/source-stamp.json')
    provenance = read(arm + '/provenance.json')
    assert stamp == provenance['source_sha256'] and len(stamp) == 95
    assert provenance['release_matches_dist']
    for name, profile in provenance['effective_manifest_profiles'].items():
        assert profile['codegen-units'] == (16 if name.endswith('-static') else 1)
    for name, digest in stamp.items():
        source = subprocess.check_output(['git', 'show', commit + ':' + name], cwd=repo)
        assert hashlib.sha256(source).hexdigest() == digest, (arm, name)
        if arm == q['runtime_candidate']: assert sha(repo / name) == digest, name
    assert '3286 passed; 0 failed; 4 ignored' in (root / arm / 'tests.log').read_text()
    acceptance = read(arm + '/acceptance.json')
    assert len(acceptance) == 40 and all(r['matches_node'] for r in acceptance)
    seeded = 0
    for name in ['all-gc-stress', 'extended-validation', 'forced-tape-validation',
                 'no-json-growth-validation', 'lifetime-fixture-validation', 'root-record-validation']:
        for r in read(arm + '/validation/' + name + '.json'):
            assert r['exit'] == 0 and r.get('live_subject', True)
            assert r.get('matches_node') or (r.get('matches_reference') and r.get('known_preexisting_mismatch_only'))
            c = r.get('counters', {})
            if c.get('scheduled_collections', 0):
                seeded += 1
                assert all(c[k] > 0 for k in ['scheduled_collections', 'copying_minors', 'moved_objects', 'loop_polls'])
    assert seeded == 52
changed = subprocess.check_output(
    ['git', 'diff', '--name-only', commits['previous'], commits['depth23'], '--', 'crates'],
    cwd=repo, text=True).splitlines()
assert set(changed) == {'crates/perry-runtime/src/json/' + name for name in [
    'parser.rs', 'parser_scan_tests.rs', 'parser_depth_string.rs', 'parser_depth_string_tests.rs']}

def quiet(folder):
    window = read(folder + '/window-custom.json')
    assert window['quiet_gate_passed']
    assert max(window['load_before'][0], window['load_after'][0]) <= 2.5
    observations = read(folder + '/trace-observations.json')
    assert observations and all(r['monitor_exit'] == 0 and not r['external'] for r in observations)

for arm in ['depth19', 'depth20', 'depth22']:
    base = 'prototypes/' + arm
    quiet(base + '/results')
    data = read(base + '/results/raw.json')
    assert len(data) == 308
    for fixture in {r['fixture'] for r in data}:
        selected = [r for r in data if r['fixture'] == fixture]
        assert len(selected) == 28 and len({r['checksum'] for r in selected}) == 1
    assert '3 passed; 0 failed' in (root / base / 'test.log').read_text()
    p = read(base + '/source-provenance.json')
    for name, digest in p.get('source_sha256', p.get('files')).items():
        assert sha(root / base / name) == digest, (arm, name)

failed = 'depth22/runtime/results'
assert not read(failed + '/window-custom.json')['quiet_gate_passed']
assert len(rows(failed + '/recheck/timing.jsonl')) == 950
assert len(rows(failed + '/recheck/verify.jsonl')) == 190
assert len(rows(failed + '/recheck/memory.jsonl')) == 360
assert len(rows(failed + '/lifetimes/raw.jsonl')) == 63
failed = 'depth23/runtime/results'
assert not read(failed + '/window-custom.json')['quiet_gate_passed']
assert len(rows(failed + '/recheck/timing.jsonl')) == 1140
assert len(rows(failed + '/recheck/verify.jsonl')) == 228
assert len(rows(failed + '/recheck/memory.jsonl')) == 480
assert len(rows(failed + '/lifetimes/raw.jsonl')) == 84
base = 'launch26/results'
quiet(base)
verified = rows(base + '/recheck/verify.jsonl')
timing = rows(base + '/recheck/timing.jsonl')
memory = rows(base + '/recheck/memory.jsonl')
life = rows(base + '/lifetimes/raw.jsonl')
assert len(verified) == 228 and all(r['correct'] for r in verified)
assert len(timing) == 1140 and len(memory) == 480 and len(life) == 84
assert all(r['exit_code'] == 0 and 'error' not in r and r['tape'] is None for r in timing + memory)
assert all(not r['trace'] for r in life)
assert all(r['output_verified'] for r in life if r['mode'] in ['latest', 'retain'])
paths = {r['argv0'] for r in timing + memory + life if r['engine'] not in ['node', 'bun']}
assert paths == {'/Users/perry/json-escape-runtime-v18-20260907-codex/active'}
assert read(base + '/recheck/metadata.json')['hashes']['candidate'] == provenance['workers']['worker']['sha256']
assert read(base + '/recheck/metadata.json')['hashes']['depth22'] == read('depth22/provenance.json')['workers']['worker']['sha256']
assert read(base + '/lifetimes/metadata.json')['workers']['candidate'] == provenance['workers']['lifetime-worker']['sha256']
summary = read(base + '/recheck/summary.json')
assert len(summary) == 38
for r in summary:
    for engine in ['checkpoint', 'previous', 'depth22', 'candidate', 'node', 'bun']:
        selected = [x for x in timing if (x['fixture'], x['operation'], x['engine']) == (r['fixture'], r['operation'], engine)]
        assert len(selected) == 5
        assert statistics.median(x['cpu_ms'] * 1000 / x['iterations'] for x in selected) == r['cpu_us'][engine]['median']
        assert statistics.median(x['peak_rss'] / 1048576 for x in selected) == r['peak_rss_mib'][engine]['median']
assert {r['name']: r['sha256'] for r in read('launch26/fixture-check.json')} == {
    r['name']: r['sha256'] for r in read(base + '/fixtures.json')}

quiet('launch25/results')
assert len(read('launch25/results/raw.json')) == 84
traces=read('launch25/results/traces.json')
assert len(traces)==4
assert all(r['cycles']==37 and r['full']==28 and r['pointer_slots']==36210217 for r in traces)
for folder in ['launch25/results','launch26/results']:
    observations=read(folder+'/trace-observations.json')
    assert not any('XprotectService' in line and float(line.split()[2])>5 for r in observations for line in r['top_cpu'])
assert len(read('launch26/results/trace-observations.json'))==79
assert read('launch26/results/recheck/metadata.json')['launcher_sha256']==read('launch25/results/metadata.json')['launcher_sha256']

for arm in ['depth23','depth24']:
    base='prototypes/'+arm
    provenance=read(base+'/source-provenance.json')
    for name,digest in provenance.get('source_sha256',provenance.get('files')).items():
        assert sha(root/base/name)==digest,(arm,name)
assert '4 passed; 0 failed' in (root/'prototypes/depth24/test-expanded.log').read_text()
selected=q['runtime_candidate']
for r in summary:
    metric=r['cpu_us']
    assert metric['delta_percent']==100*(metric[selected]['median']/metric['previous']['median']-1)
control=read('launch25/provenance.json')['files']
assert sha(root/'launch25/launch.c')==control['launch.c']
assert sha(root/'launch25/argv.ts')==control['argv.ts']
assert read('launch25/results/metadata.json')['launcher_sha256']==control['launch']
assert read('launch25/results/argv-check.json')['probe_sha256']==control['argv']
assert sum(r['cpu_us'][selected]['median']<=min(r['cpu_us'][e]['median'] for e in ['node','bun']) for r in summary)==16
assert sum(r['peak_rss_mib'][selected]['median']<=min(r['peak_rss_mib'][e]['median'] for e in ['node','bun']) for r in summary)==32
print('Depth scanner source, correctness, launch control, all 38 CPU/RSS rows, retained memory and GC lifetimes verified; performance acceptance remains open.')
