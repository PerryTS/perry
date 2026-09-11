from pathlib import Path
from collections import defaultdict
import gzip, hashlib, json, statistics, sys

work = Path(__file__).resolve().parent
bench = work.parents[1]
outputs = {}
total = 0
full_recheck = '--full-recheck' in sys.argv
recheck = True
options_only = '--options-only' in sys.argv
with_full = '--with-full' in sys.argv
assert not (recheck and options_only)
assert not (with_full and (recheck or options_only))
repetitions = 11 if recheck else 7
phases = ['access']
if with_full:
    phases += ['full']
selected_phase = sys.argv[sys.argv.index('--phase') + 1] if '--phase' in sys.argv else None
if selected_phase:
    assert selected_phase in ['access', 'focus', 'options'] and not (recheck or options_only or with_full)
    phases = [selected_phase]
for kind in phases:
    d = bench / 'results' / ('quiet-' + work.name + ('-recheck-' if recheck else '-') + kind)
    timings = [json.loads(x) for x in (d / 'timing.jsonl').read_text().splitlines()]
    verifies = [json.loads(x) for x in (d / 'verify.jsonl').read_text().splitlines()]
    summary = json.loads((d / 'summary.json').read_text())
    window = json.loads((d / 'window.json').read_text())
    assert window['quiet_gate_passed'] and window['finished_utc']
    assert json.loads((d / 'controller-exit.json').read_text())['exit_code'] == 0
    assert len(timings) == {'access': 528, 'focus': 420, 'options': 196}[kind]
    assert len(verifies) == {'access': 12, 'focus': 75, 'options': 35}[kind]
    assert not window['competing_workloads_before'] and not window['competing_workloads_after']
    assert 'results/' + d.name in window['command']
    patch = d / 'source.patch.gz'
    raw = gzip.decompress(patch.read_bytes())
    m = json.loads((d / 'source.patch.json').read_text())
    assert hashlib.sha256(raw).hexdigest() == m['original_sha256']
    assert hashlib.sha256(patch.read_bytes()).hexdigest() == m['gzip_sha256']
    assert raw == (work / 'source.patch').read_bytes()
    host = json.loads((d / 'host.json').read_text())
    remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance/'
    if kind == 'access':
        assert host['versions'] == {'node': 'v26.5.1', 'bun': '1.3.14'}
        recorded = host['hashes']
    else:
        assert host['node_version'] == 'v26.5.1' and host['bun_version'] == '1.3.14'
        recorded = host['sources']
        worker = 'options' if kind == 'options' else 'worker'
        for engine, arm in [('perry', 'candidate'), ('baseline', 'main')]:
            assert host['workers'][engine] == hashlib.sha256((work / (arm + '-' + worker)).read_bytes()).hexdigest()
    for path, digest in recorded.items():
        assert path.startswith(remote), path
        local = bench / path.removeprefix(remote)
        assert hashlib.sha256(local.read_bytes()).hexdigest() == digest, path
    fixture_hashes = json.loads((d / 'fixture-hashes.json').read_text())
    staged = json.loads((work / 'remote-stage-hashes.json').read_text())
    assert len(fixture_hashes) == 19
    for path, digest in fixture_hashes.items():
        assert staged[path] == digest == hashlib.sha256((bench / path).read_bytes()).hexdigest(), path
    expected = {(v['fixture'], v['operation']): v for v in verifies if v['engine'] == 'node'}
    for v in verifies:
        e = expected[v['fixture'], v['operation']]
        keys = ['checksum', 'retained'] if kind == 'access' else ['verify_sha256', 'checksum', 'retained']
        assert all(v[k] == e[k] for k in keys), (kind, v, e)
    groups = defaultdict(list)
    for r in timings:
        e = expected[r['fixture'], r['operation']]
        wanted = e['checksum'] / (e['iterations'] + e['warmup']) * (r['iterations'] + r['warmup'])
        assert r['checksum'] == wanted and r['retained'] == 0, (kind, r, e)
        groups[r['fixture'], r['operation'], r['engine']].append(r)
    assert len(groups) == len(summary)
    for s in summary:
        rows = groups[s['fixture'], s['operation'], s['engine']]
        assert len(rows) == repetitions and {r['rep'] for r in rows} == set(range(repetitions))
        if kind != 'access':
            assert s['repetitions'] == repetitions
        cpu = [(r['user_us'] + r['system_us']) / r['iterations'] for r in rows]
        rss = [r['peak_rss'] / 1048576 for r in rows]
        assert cpu == s['cpu_samples_us'] and rss == s['peak_rss_samples_mib']
        assert statistics.median(cpu) == s['cpu_us'] and statistics.median(rss) == s['peak_rss_mib']
    by_case = defaultdict(dict)
    for s in summary:
        assert s['engine'] not in by_case[s['fixture'], s['operation']]
        by_case[s['fixture'], s['operation']][s['engine']] = s
    assert len(by_case) == {'access': 12, 'focus': 15, 'options': 4 if recheck else 7, 'full': 5 if full_recheck else 50}[kind]
    assert all(set(e) == {'baseline', 'perry', 'node', 'bun'} for e in by_case.values())
    comparisons = []
    for (fixture, operation), engines in by_case.items():
        a, b = engines['perry'], engines['baseline']
        peers = [engines[e] for e in ['node', 'bun']]
        comparisons.append(dict(
            fixture=fixture, operation=operation, engines=engines,
            delta_pct=(a['cpu_us'] / b['cpu_us'] - 1) * 100,
            paired_delta_pct=[(x / y - 1) * 100 for x, y in zip(a['cpu_samples_us'], b['cpu_samples_us'])],
            slower_pairs=sum(x > y for x, y in zip(a['cpu_samples_us'], b['cpu_samples_us'])),
            rss_delta_mib=a['peak_rss_mib'] - b['peak_rss_mib'],
            separated_regression=min(a['cpu_samples_us']) > max(b['cpu_samples_us']),
            separated_improvement=max(a['cpu_samples_us']) < min(b['cpu_samples_us']),
            beats_both_peers=a['cpu_us'] < min(x['cpu_us'] for x in peers),
            rss_beats_both_peers=a['peak_rss_mib'] < min(x['peak_rss_mib'] for x in peers),
            all_samples_beat_both_peers=max(a['cpu_samples_us']) < min(min(x['cpu_samples_us']) for x in peers)))
    outputs[kind] = {'window': window, 'timed_trials': len(timings), 'cases': comparisons}
    total += len(timings)
assert total == 528, total
(work / 'access-recheck-analysis.json').write_text(json.dumps({'trials': total, 'phases': outputs}, indent=2) + '\n')
print('VERIFIED', total, 'timed checksums and every CPU/RSS sample vector/median; all qualified quiet windows and patches')
for kind, phase in outputs.items():
    for r in phase['cases']:
        print(kind, r['fixture'], r['operation'], f"{r['delta_pct']:+.2f}%", 'REGRESSION' if r['separated_regression'] else 'GAIN' if r['separated_improvement'] else 'overlap')
