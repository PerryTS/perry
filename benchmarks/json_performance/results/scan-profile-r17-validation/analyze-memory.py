"""Verify R17's predeclared lifetime probes; exclude the failed quiet window."""
from pathlib import Path
import hashlib
import gzip
import json
import re

w = Path(__file__).resolve().parent
bench = w.parents[1]
d = bench / 'results' / ('quiet-' + w.name + '-retry1-memory')
read = lambda name: json.loads((d / name).read_text())
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
def read_text(path):
    return path.read_text() if path.exists() else gzip.decompress(path.with_name(path.name + '.gz').read_bytes()).decode()
window = read('window.json')
assert window['quiet_gate_passed'] and window['finished_utc']
assert not window['competing_workloads_before'] and not window['competing_workloads_after']
assert 'results/' + d.name in window['command']
assert read('controller-exit.json')['exit_code'] == 0
staged = read('remote-memory-stage-hashes.json')
assert staged == json.loads((w / 'remote-memory-stage-hashes.json').read_text())
assert len(staged) == 21
for path, digest in staged.items():
    assert sha(bench / path) == digest, path

oracle = {}
for line in (bench / 'results' / ('quiet-' + w.name + '-modes') / 'verify.jsonl').read_text().splitlines():
    r = json.loads(line)
    if r['engine'] == 'oracle':
        oracle[r['fixture'], r['operation']] = r

cases = read('memory-cases.json')['cases']
records = read('memory-diagnostics.json')
assert len(cases) == len(records) == 9
assert [r['case'] for r in records] == cases
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance/'
out = []
for r in records:
    c = r['case']
    label = '-'.join([c['fixture'], c['operation'], c['mode'], str(c['iterations'])])
    expected_env = {'PERRY_GC_DIAG': '1'}
    if c['mode'] == 'main_direct':
        expected_env['PERRY_JSON_TAPE'] = '0'
    assert r['env_overrides'] == expected_env
    assert r['command'] == [remote + '.work/' + w.name + '/memory-worker',
                            remote + '.work/fixtures/' + c['fixture'] + '.json',
                            c['operation'], str(c['iterations']), '2', 'verify']
    assert r['worker_sha256'] == sha(w / 'memory-worker')
    assert r['exit_code'] == 0 and r['stop_reason'] is None
    assert 0 < r['observed_peak_rss_bytes'] <= 2 * 1024**3
    assert 0 < r['elapsed_seconds'] < 30
    stdout = read_text(d / (label + '.stdout'))
    result = list(map(float, re.search(r'^RESULT (.+)$', stdout, re.M)[1].split()))
    expected = oracle[c['fixture'], c['operation']]
    assert len(result) == 7 and result[6] == 0
    assert result[5] == expected['checksum'] / 15 * (c['iterations'] + 2)
    verify = stdout.split('\nVERIFY ', 1)[1].split('\nKEEP ', 1)[0]
    post_gc = stdout.split('\nPOST_GC_VERIFY ', 1)[1].split('\nMEMORY_COLLECTED_DROPPED ', 1)[0]
    for value in (verify, post_gc):
        assert hashlib.sha256(value.encode()).hexdigest() == expected['verify_sha256'], label
    rss = {name: int(re.search(r'^' + name + r' (\d+)$', stdout, re.M)[1])
           for name in ('MEMORY_LIVE', 'MEMORY_COLLECTED_LIVE', 'MEMORY_COLLECTED_DROPPED')}
    assert all(0 < value <= 2 * 1024**3 for value in rss.values())
    stderr = read_text(d / (label + '.stderr'))
    all_full = re.findall(r'^\[gc-full\] site=(\w+) trigger=(\w+) count_at_site=(\d+) old_reclaimable=(\d+) old_baseline=(\d+)$', stderr, re.M)
    all_freed = list(map(int, re.findall(r'^\[gc\] blocks:.*? freed_bytes=(\d+) ', stderr, re.M)))
    assert len(all_full) == len(all_freed), label
    manual = [(v, size) for v, size in zip(all_full, all_freed) if v[1] == 'Manual']
    assert [(v[0], int(v[2])) for v, size in manual] == [('sync', 1), ('sync', 2)], label
    freed = [size for v, size in manual]
    assert len(freed) == 2 and all(value > 0 for value in freed), label
    exit_line = re.search(r'^\[gc-arena-right-size\] (.+)$', stderr, re.M)[1]
    arena = {key: int(value) for key, value in re.findall(r'(arena_live|arena_capacity)=(\d+)', exit_line)}
    assert 0 < arena['arena_live'] <= arena['arena_capacity']
    copying = len(re.findall(r'^\[gc-copy-minor\] ran ', stderr, re.M))
    declared_copying = int(re.search(r'copying_minors=(\d+)', stderr)[1])
    assert copying == declared_copying
    origins = [{'minor': int(minor), 'type': kind, 'objects': int(objects),
                'bytes': int(size), 'promoted_bytes': int(promoted)}
               for minor, kind, objects, size, promoted in re.findall(
                   r'^\[gc-survival\]   minor=(\d+) origin=remembered_set/lazy_array type=(\w+) objects=(\d+) bytes=(\d+) promoted_bytes=(\d+)$', stderr, re.M)]
    row = {'case': c, 'full_output_matches_node_before_and_after_gc': True,
           'rss_after_timing_bytes': int(result[4]), 'rss_snapshots_bytes': rss,
           'observed_peak_rss_bytes': r['observed_peak_rss_bytes'],
           'manual_full_collections': 2, 'full_collection_freed_bytes': freed,
           'pre_manual_full_old_reclaimable_bytes': [int(v[3]) for v, size in manual],
           'all_full_collections': [{'site': v[0], 'trigger': v[1], 'freed_bytes': size}
                                    for v, size in zip(all_full, all_freed)],
           'copying_minors': copying, 'sampled_lazy_owner_survival': origins,
           'final_arena': arena, 'stdout_sha256': hashlib.sha256(stdout.encode()).hexdigest(),
           'stderr_sha256': hashlib.sha256(stderr.encode()).hexdigest()}
    out.append(row)
    print(label, 'RSS MiB', round(result[4] / 1048576, 2),
          [round(v / 1048576, 2) for v in rss.values()],
          'first full freed MiB', round(freed[0] / 1048576, 2),
          'final arena live MiB', round(arena['arena_live'] / 1048576, 2))

(w / 'memory-analysis.json').write_text(json.dumps({
    'window': window, 'diagnostic_only': True, 'instrumented': True,
    'excluded_window': 'quiet-scan-profile-r17-memory',
    'note': 'Explicit GC occurs after timed work. MEMORY_LIVE follows output verification; RESULT RSS precedes it. '
            'Snapshots and monitor peaks are not ru_maxrss or speedup measurements. '
            'Full collections reclaimed bytes; RSS remaining high does not imply those objects are still live. '
            'These probes establish no protected-from-space safety verdict.',
    'cases': out}, indent=2) + '\n')
print('VERIFIED nine complete outputs before/after live GC, 18 productive manual full collections, all 21 hashes and quiet window.')
