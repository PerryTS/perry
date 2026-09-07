#!/usr/bin/env python3
"""Verify pinned source, validation, measurement qualification and summaries."""
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

for name, digest in read('manifest.json').items():
    assert sha(root / name) == digest, name
q = read('qualification.json')
assert q['experimental'] and not q['no_regression_acceptance']
assert q['all_38_complete'] and not q['older_full_inventory_complete']
assert not q['gc_policy_changed']
assert not q['promoted_over_previous']
commits = read('source-commit.json')
for arm, commit, count in [('baseline', commits['previous'], 95),
                           ('candidate', commits['candidate'], 97)]:
    stamp = read(arm + '/source-stamp.json')
    p = read(arm + '/provenance.json')
    assert stamp == p['source_sha256'] and len(stamp) == count
    assert p['release_matches_dist']
    for name, profile in p['effective_manifest_profiles'].items():
        assert profile['codegen-units'] == (16 if name.endswith('-static') else 1)
    for name, digest in stamp.items():
        source = subprocess.check_output(['git', 'show', commit + ':' + name], cwd=repo)
        assert hashlib.sha256(source).hexdigest() == digest, (arm, name)
        if arm == 'candidate':
            assert sha(repo / name) == digest, name
changed = subprocess.check_output(
    ['git', 'diff', '--name-only', commits['previous'], commits['candidate'], '--', 'crates'],
    cwd=repo, text=True).splitlines()
assert set(changed) == {'crates/perry-runtime/src/json/' + name for name in [
    'parser.rs', 'parser_depth_blocks.rs', 'parser_depth_blocks_tests.rs']}
assert '3288 passed; 0 failed; 4 ignored' in (root / 'candidate/tests.log').read_text()
acceptance = read('candidate/acceptance.json')
assert len(acceptance) == 40 and all(r['matches_node'] for r in acceptance)
seeded = 0
for name in ['all-gc-stress', 'extended-validation', 'forced-tape-validation',
             'no-json-growth-validation', 'lifetime-fixture-validation', 'root-record-validation']:
    for r in read('candidate/validation/' + name + '.json'):
        assert r['exit'] == 0 and r.get('live_subject', True)
        assert r.get('matches_node') or (r.get('matches_reference') and r.get('known_preexisting_mismatch_only'))
        c = r.get('counters', {})
        if c.get('scheduled_collections', 0):
            seeded += 1
            assert all(c[k] > 0 for k in ['scheduled_collections', 'copying_minors', 'moved_objects', 'loop_polls'])
assert seeded == 52

failed = 'launch27/results/'
assert len(rows(failed + 'recheck/timing.jsonl')) == 950
assert len(rows(failed + 'recheck/verify.jsonl')) == 190
assert len(rows(failed + 'recheck/memory.jsonl')) == 360
assert len(rows(failed + 'lifetimes/raw.jsonl')) == 63
assert sum(bool(r['xprotect_busy']) for r in read(failed + 'trace-observations.json')) == 1
assert not read('launch27/diagnostic-summary.json')['qualified']

base = 'launch29/results/'
window = read(base + 'window-custom.json')
assert window['quiet_gate_passed']
assert max(window['load_before'][0], window['load_after'][0]) <= 2.5
observations = read(base + 'trace-observations.json')
assert observations and all(r['monitor_exit'] == 0 and not r['external'] and not r['xprotect_busy'] for r in observations)
assert len(observations) == q['quiet_observations']
verified = rows(base + 'recheck/verify.jsonl')
timing = rows(base + 'recheck/timing.jsonl')
memory = rows(base + 'recheck/memory.jsonl')
life = rows(base + 'lifetimes/raw.jsonl')
assert len(verified) == 190 and all(r['correct'] for r in verified)
assert len(timing) == 950 and len(memory) == 360 and len(life) == 63
assert all(r['exit_code'] == 0 and 'error' not in r and r['tape'] is None for r in timing + memory)
assert all(not r['trace'] for r in life)
assert all(r['output_verified'] for r in life if r['mode'] in ['latest', 'retain'])
paths = {r['argv0'] for r in timing + memory + life if r['engine'] not in ['node', 'bun']}
assert paths == {'/Users/perry/json-escape-runtime-v18-20260907-codex/active'}
metadata = read(base + 'recheck/metadata.json')
life_meta = read(base + 'lifetimes/metadata.json')
for engine, arm in [('previous', 'baseline'), ('candidate', 'candidate')]:
    p = read(arm + '/provenance.json')
    assert metadata['hashes'][engine] == p['workers']['worker']['sha256']
    assert life_meta['workers'][engine] == p['workers']['lifetime-worker']['sha256']
launcher = read('../json-depth-scanning/launch25/results/metadata.json')['launcher_sha256']
assert metadata['launcher_sha256'] == launcher
summary = read(base + 'recheck/summary.json')
assert len(summary) == 38
for r in summary:
    for engine in ['checkpoint', 'previous', 'candidate', 'node', 'bun']:
        selected = [x for x in timing if (x['fixture'], x['operation'], x['engine']) == (r['fixture'], r['operation'], engine)]
        assert len(selected) == 5
        assert statistics.median(x['cpu_ms'] * 1000 / x['iterations'] for x in selected) == r['cpu_us'][engine]['median']
        assert statistics.median(x['peak_rss'] / 1048576 for x in selected) == r['peak_rss_mib'][engine]['median']
    c = r['cpu_us']
    assert c['delta_percent'] == 100 * (c['candidate']['median'] / c['previous']['median'] - 1)
assert sum(r['cpu_us']['candidate']['median'] <= min(r['cpu_us'][e]['median'] for e in ['node', 'bun']) for r in summary) == q['cpu_rows_at_or_below_both']
assert sum(r['peak_rss_mib']['candidate']['median'] <= min(r['peak_rss_mib'][e]['median'] for e in ['node', 'bun']) for r in summary) == q['rss_rows_at_or_below_both']
assert {r['name']: r['sha256'] for r in read('launch29/fixture-check.json')} == {
    r['name']: r['sha256'] for r in read(base + 'fixtures.json')}

for r in read(base + 'recheck/memory-summary.json'):
    selected = [x for x in memory if (x['fixture'], x['operation'], x['iterations']) == (r['fixture'], r['operation'], r['iterations'])]
    assert len(selected) == 9
    for engine in ['checkpoint', 'previous', 'candidate']:
        for metric in ['peak_rss', 'rss_after']:
            assert statistics.median(x[metric] / 1048576 for x in selected if x['engine'] == engine) == r['values'][engine][metric]['median']
for r in read(base + 'lifetimes/summary.json'):
    selected = [x for x in life if (x['fixture'], x['operation'], x['mode'], x['iterations']) == (r['fixture'], r['operation'], r['mode'], r['iterations'])]
    assert len(selected) == 9
    for engine in ['checkpoint', 'previous', 'candidate']:
        for metric in ['totalCpuUs', 'peak_rss', 'rssDrained']:
            assert statistics.median(x[metric] for x in selected if x['engine'] == engine) == r['values'][engine][metric]['median']
diag = 'diag30/results/'
assert read(diag + 'window-custom.json')['quiet_gate_passed']
assert all(r['monitor_exit'] == 0 and not r['external'] and not r['xprotect_busy']
           for r in read(diag + 'trace-observations.json'))
targeted = rows(diag + 'timing.jsonl')
assert len(targeted) == 56 and all(r['exit_code'] == 0 and not r['trace'] for r in targeted)
assert read(diag + 'metadata.json')['hashes'] == {
    e: metadata['hashes'][e] for e in ['previous', 'candidate']}
for r in read(diag + 'summary.json'):
    for engine in ['previous', 'candidate']:
        chosen = [x for x in targeted if (x['fixture'], x['engine']) == (r['fixture'], engine)]
        assert len(chosen) == 7
        assert statistics.median(x['cpu_ms'] * 1000 / x['iterations'] for x in chosen) == r['values'][engine]['cpu_us_median']
        assert statistics.median(x['peak_rss'] / 1024 for x in chosen) == r['values'][engine]['peak_kib_median']
    if r['fixture'] in ['null', 'string_a']:
        assert r['cpu_change_pct'] > 2 and r['cpu_ranges_separated']
traces = read(diag + 'traces.json')
assert len(traces) == 4
for r in traces:
    events = rows(diag + r['trace_file'])
    assert len(events) == r['cycles'] == r['full'] == 1
    assert sum(x['layout_scans']['pointer_slots_read'] for x in events) == r['pointer_slots'] == 20777
for r in read(diag + 'gc-retention-summary.json'):
    event = rows(diag + f"{r['engine']}-{r['rep']}.trace.jsonl")[0]
    assert r['roots'] == event['conservative_root_count']
    assert r['before_live_allocated_bytes'] == event['arena_bytes']['before']['total_live_allocated_bytes'] == 34442896
    assert r['after_live_allocated_bytes'] == event['arena_bytes']['after']['total_live_allocated_bytes']
print('Source, validation, rejected/qualified windows, all 38 CPU/RSS rows, lifetimes and confirmed regressions verified; candidate remains experimental.')
