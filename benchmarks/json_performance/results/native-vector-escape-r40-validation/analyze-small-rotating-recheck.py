from collections import defaultdict
from pathlib import Path
import gzip, hashlib, json, statistics

w = Path(__file__).resolve().parent
bench = w.parents[1]
d = bench / 'results' / ('quiet-' + w.name + '-small-recheck-rotating')
read = lambda name: json.loads((d / name).read_text())
rows = lambda name: [json.loads(x) for x in (d / name).read_text().splitlines()]
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
window = read('window.json')
assert window['quiet_gate_passed'] and window['finished_utc']
assert json.loads((d / 'controller-exit.json').read_text())['exit_code'] == 0
assert not window['competing_workloads_before'] and not window['competing_workloads_after']
assert 'results/' + d.name in window['command']
host = read('host.json')
assert host['versions'] == {'node': 'v26.5.1', 'bun': '1.3.14'}
assert host['worker_sha256'] == sha(w / 'candidate-rotating-worker')
assert host['baseline_worker_sha256'] == sha(w / 'main-rotating-worker')
assert host['commit'] == json.loads((w / 'provenance.json').read_text())['source_commit']
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance/'
for path, digest in host['source_sha256'].items():
    assert path.startswith(remote)
    assert sha(bench / path.removeprefix(remote)) == digest
stage = json.loads((w / 'remote-stage-hashes.json').read_text())
for f in host['fixtures']:
    for i, digest in enumerate(f['sha256']):
        path = '.work/rotating/' + f['fixture'] + '.' + str(i) + '.json'
        assert stage[path] == digest == sha(bench / path)
patch = gzip.decompress((d / 'source.patch.gz').read_bytes())
assert patch == (w / 'source.patch').read_bytes()
pm = read('source.patch.json')
assert hashlib.sha256(patch).hexdigest() == pm['original_sha256']
assert sha(d / 'source.patch.gz') == pm['gzip_sha256']
verify = rows('verify.jsonl')
expected = {(x['fixture'], x['mode'], x['iterations']): x for x in verify if x['engine'] == 'node'}
for x in verify:
    e = expected[x['fixture'], x['mode'], x['iterations']]
    assert x['correct'] and x['exit_code'] == 0
    assert x['verified_members'] == list(range(8))
    assert all(x[k] == e[k] for k in ['checksum', 'last_sha256', 'verify_sha256'])
timing = rows('timing.jsonl')
calibration = rows('calibration.jsonl')
assert len(timing) == 132 and len(calibration) == 12 and len(verify) == 20
for x in timing + calibration:
    e = expected[x['fixture'], x['mode'], 9]
    assert x['exit_code'] == 0 and x['retained'] == 0 and x['peak_rss'] > 0
    unit = e['checksum'] / (e['iterations'] + e['warmup'])
    assert x['checksum'] == unit * (x['iterations'] + x['warmup'])
groups = defaultdict(list)
for x in timing:
    groups[x['fixture'], x['mode'], x['engine']].append(x)
cases = defaultdict(dict)
for (fixture, mode, engine), samples in groups.items():
    samples.sort(key=lambda x: x['rep'])
    assert [x['rep'] for x in samples] == list(range(11))
    assert len({x['iterations'] for x in samples}) == 1
    cpu = [(x['user_us'] + x['system_us']) / x['iterations'] for x in samples]
    rss = [x['peak_rss'] / 1048576 for x in samples]
    cases[fixture, mode][engine] = dict(iterations=samples[0]['iterations'], cpu_samples_us=cpu,
        cpu_us=statistics.median(cpu), peak_rss_samples_mib=rss, peak_rss_mib=statistics.median(rss))
output = []
for (fixture, mode), engines in cases.items():
    assert set(engines) == {'perry', 'baseline', 'node', 'bun'}
    assert len({e['iterations'] for e in engines.values()}) == 1
    a, b = engines['perry'], engines['baseline']
    delta = (a['cpu_us'] / b['cpu_us'] - 1) * 100
    slower = sum(x > y for x, y in zip(a['cpu_samples_us'], b['cpu_samples_us']))
    regression = min(a['cpu_samples_us']) > max(b['cpu_samples_us'])
    improvement = max(a['cpu_samples_us']) < min(b['cpu_samples_us'])
    output.append(dict(fixture=fixture, mode=mode, engines=engines, delta_pct=delta,
        slower_pairs=slower, separated_regression=regression, separated_improvement=improvement))
    print(f'{fixture} {mode}: {delta:+.2f}%, {slower}/11 slower; ' +
          ('REGRESSION' if regression else 'GAIN' if improvement else 'overlap'))
assert len(output) == 3
(w / 'small-rotating-recheck-analysis.json').write_text(json.dumps(dict(trials=132, calibration_trials=12,
    verification_trials=20, window=window, cases=output), indent=2) + '\n')
print('Verified all 132 timing, 12 calibration, 20 full-output checks, hashes and quiet window.')
