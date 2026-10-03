"""Attribute paired TS instructions to user/kernel execution; diagnostic only.
Uses already verified immutable full-runtime executables. No compiler, policy
or GC environment changes. Never replaces the application acceptance matrices.
"""
import importlib.util
import json
from pathlib import Path
import statistics
import subprocess

VERSION = 'gc-paired-instruction-domains-v1'
EVENTS = ('instructions:u', 'instructions:k', 'minor-faults', 'major-faults')
PLANS = (
    ('gc-pool-v3-noauto', 'gc-runtime-pool-v3r1-noauto.exit',
     ('gc-runtime-scan-main', 'gc-runtime-pool-r1-main')),
    ('gc-inline-v4-noauto', 'gc-runtime-inline-v4r1-noauto.exit',
     ('gc-runtime-pool-r1-main', 'gc-runtime-inline-main')),
)


def parse_events(stderr):
    counters = {}
    for line in stderr.splitlines():
        fields = line.split(',')
        if len(fields) < 5 or fields[2] not in EVENTS:
            continue
        name = fields[2]
        assert name not in counters, ('duplicate counter', name)
        assert fields[0].isdigit(), ('unavailable counter', name, fields[0])
        assert float(fields[4]) >= 99.0, ('multiplexed counter', name, fields[4])
        counters[name] = int(fields[0])
    assert set(counters) == set(EVENTS), ('incomplete events', counters)
    assert counters['instructions:u'] > 0
    assert counters['instructions:k'] > 0, 'kernel domain was not observed'
    return counters


def run():
    import bench
    b = bench.B
    assert not (b / (VERSION + '-runs.json')).exists(), 'preserve partial/completed evidence'
    module_spec = importlib.util.spec_from_file_location('paired_summary', b / 'summarize-gc-comparison.py')
    summary_module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(summary_module)
    source = b / 'sources/real/tscwork.ts'
    subjects = []
    inputs = {}
    for prefix, receipt, arms in PLANS:
        subprocess.run(['python3', str(b / 'summarize-gc-comparison.py'), str(b / prefix),
                        '--arms', *arms, '--suite', 'noauto', '--complete', str(b / receipt)], check=True)
        paths = {name: b / (prefix + '-' + name + '.json') for name in ('runs', 'oracle', 'builds', 'products')}
        inputs[prefix] = {str(p): bench.m.sha(p) for p in [*paths.values(), b / receipt]}
        measured = json.loads(paths['runs'].read_text())
        builds = json.loads(paths['builds'].read_text())
        products = json.loads(paths['products'].read_text())
        assert all(bench.m.sha(Path(p)) == digest for p, digest in products.items())
        oracle = json.loads(paths['oracle'].read_text())['tscwork']
        for arm in arms:
            cells = [r for r in measured if r['case'] == 'tscwork' and r['arm'] == arm]
            build = [r for r in builds if r['case'] == 'tscwork' and r['arm'] == arm]
            assert len(build) == 1
            build = build[0]
            assert build['source_sha256'] == bench.m.sha(source)
            assert len(cells) == 6 and all(r['stdout'] == oracle for r in cells)
            binary = b / 'bin' / f'{prefix}-{arm}-tscwork'
            assert bench.m.sha(binary) == build['binary_sha256']
            subjects.append(dict(prefix=prefix, arm=arm, binary=str(binary),
                                 binary_sha256=build['binary_sha256'], expected_stdout=oracle))
    bench.m.save(VERSION + '-inputs.json', inputs)
    # Check the oracle at execution time as well as its prior manifest identity.
    node = bench.run(VERSION + '-node', ['node', '--expose-gc', source, '1'], timeout=300)
    assert node['rc'] == 0 and node['reason'] is None
    assert all(node['stdout'] == s['expected_stdout'] for s in subjects)
    records = []
    for repeat in range(3):
        for subject in subjects if repeat % 2 == 0 else reversed(subjects):
            label = f"{VERSION}-{subject['prefix']}-{subject['arm']}-{repeat}"
            result = bench.run(label, ['taskset', '-c', '2', 'perf', 'stat', '-x,',
                '-e', ','.join(EVENTS), subject['binary'], '1'], timeout=300)
            result.update(subject, repeat=repeat)
            result['correct'] = result['rc'] == 0 and result['reason'] is None and result['stdout'] == subject['expected_stdout']
            records.append(result)
            bench.m.save(VERSION + '-runs.json', records)
            assert result['correct'], label
            assert bench.m.sha(Path(subject['binary'])) == subject['binary_sha256']
            result['events'] = parse_events((b / 'logs' / (label + '.err')).read_text())
            bench.m.save(VERSION + '-runs.json', records)
            print(subject['prefix'], subject['arm'], repeat, result['events'], flush=True)
    assert len(records) == 12
    for hashes in inputs.values():
        assert all(bench.m.sha(Path(p)) == h for p, h in hashes.items())
    comparisons = []
    for prefix, _, arms in PLANS:
        metrics = {}
        for arm in arms:
            cells = [r for r in records if r['prefix'] == prefix and r['arm'] == arm]
            assert len(cells) == 3
            metrics[arm] = {event: summary_module.distribution([r['events'][event] for r in cells]) for event in EVENTS}
        changes = {}
        for event in EVENTS:
            a, c = (metrics[arm][event]['median'] for arm in arms)
            changes[event] = dict(absolute=c-a, percent=100*(c/a-1) if a else None)
        comparisons.append(dict(prefix=prefix, baseline=arms[0], candidate=arms[1], measurements=metrics, change=changes))
    bench.m.save(VERSION + '-summary.json', dict(
        scope='Paired full-runtime TypeScript user/kernel instruction counts. Three repetitions on a shared host. Diagnostic only; separate runs from the original RSS/total-instruction matrices. Kernel counts include page-fault handling and other kernel work, not a direct fault-handler attribution.',
        complete_cells=len(records), inputs=inputs, comparisons=comparisons))


if __name__ == '__main__':
    run()
