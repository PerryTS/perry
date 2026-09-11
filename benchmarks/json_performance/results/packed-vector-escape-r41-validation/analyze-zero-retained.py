from pathlib import Path
from collections import defaultdict
import gzip, hashlib, json, statistics

w = Path(__file__).resolve().parent
bench = w.parents[1]
d = bench / 'results' / ('quiet-' + w.name + '-zero-retained')
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
read = lambda name: json.loads((d / name).read_text())
rows = lambda name: [json.loads(x) for x in (d / name).read_text().splitlines()]
window = read('window.json')
assert window['quiet_gate_passed'] and window['finished_utc']
assert json.loads((d / 'controller-exit.json').read_text())['exit_code'] == 0
assert not window['competing_workloads_before'] and not window['competing_workloads_after']
assert 'results/' + d.name in window['command']
host = read('host.json')
assert host['node_version'] == 'v26.5.1' and host['bun_version'] == '1.3.14'
for engine, arm in [('perry', 'candidate'), ('baseline', 'main')]:
    assert host['workers'][engine] == sha(w / (arm + '-retained-zero'))
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance/'
for path, digest in host['sources'].items():
    assert path.startswith(remote) and sha(bench / path.removeprefix(remote)) == digest
staged = json.loads((w / 'remote-stage-hashes.json').read_text())
assert all(sha(bench / path) == digest for path, digest in staged.items())
fixtures = read('fixture-hashes.json')
assert len(fixtures) == 19
for path, digest in fixtures.items():
    assert staged[path] == digest == sha(bench / path)
raw = gzip.decompress((d / 'source.patch.gz').read_bytes())
assert raw == (w / 'source.patch').read_bytes()
patch = read('source.patch.json')
assert hashlib.sha256(raw).hexdigest() == patch['original_sha256']
assert sha(d / 'source.patch.gz') == patch['gzip_sha256']
verification, memory, summary = rows('verify.jsonl'), rows('memory.jsonl'), read('summary.json')
assert len(verification) == 40 and len(memory) == 224 and len(summary) == 32
expected = {(r['fixture'], r['operation'], r['case_iterations']): r
            for r in verification if r['engine'] == 'node'}
assert len(expected) == 8
for r in verification:
    e = expected[r['fixture'], r['operation'], r['case_iterations']]
    assert all(r[k] == e[k] for k in ['checksum', 'retained', 'verify_sha256'])
    assert r['retained'] == r['iterations'] + r['warmup']
groups = defaultdict(list)
for r in memory:
    e = expected[r['fixture'], r['operation'], r['iterations']]
    assert r['warmup'] == 0 and r['retained'] == r['iterations']
    assert r['checksum'] == e['checksum'] / (e['iterations'] + e['warmup']) * r['iterations']
    assert r['peak_rss'] > 0 and r['rss_after'] > 0 and r['rss_before'] > 0
    groups[r['fixture'], r['operation'], r['iterations'], r['engine']].append(r)
cases = defaultdict(dict)
for s in summary:
    key = s['fixture'], s['operation'], s['iterations'], s['engine']
    chosen = sorted(groups[key], key=lambda r: r['rep'])
    assert [r['rep'] for r in chosen] == list(range(7)) and s['repetitions'] == 7
    metrics = dict(cpu=[(r['user_us'] + r['system_us']) / r['iterations'] for r in chosen],
                   peak_rss=[r['peak_rss'] / 1048576 for r in chosen],
                   rss_after=[r['rss_after'] / 1048576 for r in chosen],
                   rss_before=[r['rss_before'] / 1048576 for r in chosen])
    for name, values in metrics.items():
        unit = 'us' if name == 'cpu' else 'mib'
        assert values == s[name + '_samples_' + unit]
        assert statistics.median(values) == s[name + '_' + unit]
    assert s['engine'] not in cases[key[:3]]
    cases[key[:3]][s['engine']] = s
out = []
for (fixture, operation, count), engines in cases.items():
    assert set(engines) == {'perry', 'baseline', 'node', 'bun'}
    a, b = engines['perry'], engines['baseline']
    out.append(dict(fixture=fixture, operation=operation, count=count, engines=engines,
                    cpu_delta_pct=(a['cpu_us'] / b['cpu_us'] - 1) * 100,
                    cpu_slower_pairs=sum(x > y for x, y in zip(a['cpu_samples_us'], b['cpu_samples_us'])),
                    cpu_separated_regression=min(a['cpu_samples_us']) > max(b['cpu_samples_us']),
                    rss_after_delta_mib=a['rss_after_mib'] - b['rss_after_mib'],
                    peak_rss_delta_mib=a['peak_rss_mib'] - b['peak_rss_mib'],
                    rss_after_separated_regression=min(a['rss_after_samples_mib']) > max(b['rss_after_samples_mib']),
                    rss_after_beats_both_peers=a['rss_after_mib'] < min(engines[e]['rss_after_mib'] for e in ['node', 'bun'])))
assert len(out) == 8
(w / 'zero-retained-analysis.json').write_text(json.dumps(dict(window=window, trials=224,
    verification_trials=40, note='Whole-process RSS with live retained outputs, not isolated heap size.', cases=out), indent=2) + '\n')
print('Verified all 224 retained-output trials, 40 complete-output checks, all vectors and provenance.')
