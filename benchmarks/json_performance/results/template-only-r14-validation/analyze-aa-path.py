from pathlib import Path
from collections import defaultdict
import hashlib, json, statistics

w = Path(__file__).resolve().parent
bench = w.parents[1]
d = bench / 'results' / ('quiet-' + w.name + '-aa-path-full')
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
read = lambda name: json.loads((d / name).read_text())
rows = lambda name: [json.loads(x) for x in (d / name).read_text().splitlines()]
window, host, provenance = read('window.json'), read('host.json'), read('provenance.json')
assert window['quiet_gate_passed'] and window['finished_utc']
assert not window['competing_workloads_before'] and not window['competing_workloads_after']
cmd = window['command']; assert len(cmd[cmd.index('--worker') + 1]) == len(cmd[cmd.index('--baseline-worker') + 1]) + 5
assert host['workers']['perry'] == host['workers']['baseline'] == sha(w / 'main-worker')
assert provenance['candidate_measured'] is False
assert provenance['source_commit'] == json.loads((w / 'main-build-provenance.json').read_text())['source_commit']
assert host['node_version'] == 'v26.5.1' and host['bun_version'] == '1.3.14'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance/'
for path, digest in host['sources'].items():
    assert path.startswith(remote) and sha(bench / path.removeprefix(remote)) == digest
for path, digest in read('fixture-hashes.json').items():
    assert sha(bench / path) == digest
timing, verify, summary = rows('timing.jsonl'), rows('verify.jsonl'), read('summary.json')
assert len(timing) == 88 and len(verify) == 10 and len(summary) == 8
expected = {(r['fixture'], r['operation']): r for r in verify if r['engine'] == 'node'}
for r in verify:
    e = expected[r['fixture'], r['operation']]
    assert all(r[k] == e[k] for k in ['checksum', 'retained', 'verify_sha256'])
groups = defaultdict(list)
for r in timing:
    e = expected[r['fixture'], r['operation']]
    assert r['retained'] == 0 and r['peak_rss'] > 0
    assert r['checksum'] == e['checksum'] / (e['iterations'] + e['warmup']) * (r['iterations'] + r['warmup'])
    groups[r['fixture'], r['operation'], r['engine']].append(r)
cases = defaultdict(dict)
for s in summary:
    sample = sorted(groups[s['fixture'], s['operation'], s['engine']], key=lambda r: r['rep'])
    assert [r['rep'] for r in sample] == list(range(11))
    cpu = [(r['user_us'] + r['system_us']) / r['iterations'] for r in sample]
    rss = [r['peak_rss'] / 1048576 for r in sample]
    assert cpu == s['cpu_samples_us'] and statistics.median(cpu) == s['cpu_us']
    assert rss == s['peak_rss_samples_mib'] and statistics.median(rss) == s['peak_rss_mib']
    cases[s['fixture'], s['operation']][s['engine']] = s
out = []
for (fixture, operation), engines in cases.items():
    assert set(engines) == {'perry', 'baseline', 'node', 'bun'}
    a, b = engines['perry'], engines['baseline']
    row = dict(fixture=fixture, operation=operation, engines=engines,
               delta_pct=(a['cpu_us'] / b['cpu_us'] - 1) * 100,
               slower_pairs=sum(x > y for x, y in zip(a['cpu_samples_us'], b['cpu_samples_us'])),
               separated=min(a['cpu_samples_us']) > max(b['cpu_samples_us']) or max(a['cpu_samples_us']) < min(b['cpu_samples_us']))
    out.append(row); print('A/A path', fixture, operation, row['delta_pct'], row['slower_pairs'], 'separated', row['separated'])
(w / 'aa-path-analysis.json').write_text(json.dumps(dict(window=window, trials=88, verification_trials=10,
    candidate_measured=False, cases=out), indent=2) + '\n')
print('Verified identical-main/different-name A/A control: 88 timings and 10 complete-output checks.')
