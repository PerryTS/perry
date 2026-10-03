"""Verify frozen Linux page-compaction evidence and extract boundary accounting.

Run on the measurement host with its evidence directory as the sole argument.
This checks measured inputs; it does not accept the current PR combination.
"""
import hashlib
import json
from pathlib import Path
import re
import statistics
import subprocess
import sys

B = Path(sys.argv[1])
ARM = 'gc-runtime-pages-main'
CONTROL = 'gc-runtime-inline-main'


def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


def read(name):
    return json.loads((B / name).read_text())


def save(name, data):
    (B / name).write_text(json.dumps(data, indent=2) + '\n')


receipts = {}
for stage in ['build', 'linked', 'noauto', 'census', 'auto', 'run']:
    p = B / f'gc-runtime-pages-v5r2-{stage}.exit'
    assert p.read_text().strip() == '0', p
    receipts[p.name] = sha(p)

products = read('gc-runtime-pages-main-products.json')
for name, digest in products.items():
    assert sha(B / ARM / name) == digest, name
assert read('gc-pages-v5-linked-products.json') == {
    str(B / ARM / n): h for n, h in products.items()
}
artifact = read('gc-runtime-pages-v5r2-artifact-verification.json')
assert artifact['archive_sha256'] == products['libperry_runtime.a']
assert artifact['archive_sha256'] != artifact['preceding_archive_sha256']
assert artifact['decommit_symbol_witness'] is True
logs = {}
counts = {}
for suffix, expected in [('tests', (4830, 0, 5)), ('ffi', (75, 0, 0)),
                         ('events', (15, 0, 0))]:
    p = B / 'logs' / f'gc-runtime-pages-v5r2-linux-{suffix}.log'
    text = p.read_text()
    found = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored', text)
    assert found and tuple(map(int, found[-1])) == expected, suffix
    counts[suffix] = expected
    logs[str(p)] = sha(p)
    if suffix == 'tests':
        assert sha(p) == artifact['test_log_sha256']
        for test in artifact['required_tests']:
            assert re.search(r'test [^\n]*::' + re.escape(test) + r' \.\.\. ok', text), test

node = '/opt/node-v26.5.1-linux-x64/bin/node'
assert subprocess.check_output([node, '--version'], text=True).strip() == 'v26.5.1'
linked_rows = read('gc-pages-v5-linked-runs.json')
cases = ['test_gap_gc_container_value_rooting',
         'test_gap_gc_define_properties_key_rooting',
         'test_gap_gc_store_ic_old_to_young']
assert len(linked_rows) == 9
linked = []
for case in cases:
    source = B / 'perry-gc-runtime-main' / 'test-files' / (case + '.ts')
    builds = [r for r in linked_rows if r['case'] == case and r['mode'] == 'build']
    assert len(builds) == 1
    build = builds[0]
    assert build['rc'] == 0 and build['reason'] is None
    assert build['source_sha256'] == sha(source)
    oracle = subprocess.check_output([node, '--expose-gc', str(source)], text=True)
    for mode in ['default', 'moving']:
        rows = [r for r in linked_rows if r['case'] == case and r['mode'] == mode]
        assert len(rows) == 1
        r = rows[0]
        assert r['rc'] == 0 and r['reason'] is None and r['correct'] is True
        assert sha(r['cmd'][0]) == r['binary_sha256']
        assert r['stdout'] == oracle
        assert (B / 'logs' / (r['label'] + '.out')).read_text() == oracle
        item = dict(case=case, mode=mode, binary_sha256=r['binary_sha256'],
                    oracle_sha256=hashlib.sha256(oracle.encode()).hexdigest())
        if mode == 'moving':
            lines = (B / 'logs' / (r['label'] + '.err')).read_text().splitlines()
            cycles = [json.loads(s) for s in lines if s.startswith('{')]
            copied = sum(c.get('copying_nursery', {}).get('copied_objects', 0) for c in cycles)
            protected = any(s.startswith('[gc-fromspace-protect]') and 'retired_set=' in s for s in lines)
            assert copied == r['copied_objects'] and copied > 0
            assert protected is r['protected'] is True
            item.update(copied_objects=copied, protected=protected)
        linked.append(item)

observations = read('gc-pages-v5-census-observations.json')
census_runs = read('gc-pages-v5-census-runs.json')
assert len(observations) == 8 and len(census_runs) == 10
assert {(o['arm'], o['case'], o['repeat']) for o in observations} == {
    (arm, case, repeat) for arm in [CONTROL, ARM]
    for case in ['tscwork', 'zodwork'] for repeat in range(2)
}
oracles = read('gc-pages-v5-noauto-oracle.json')
snapshots = []
for o in observations:
    rows = [r for r in census_runs if r['mode'] == 'census' and
            (r['arm'], r['case'], r['repeat']) == (o['arm'], o['case'], o['repeat'])]
    assert len(rows) == 1
    r = rows[0]
    assert r['rc'] == 0 and r['reason'] is None and r['correct'] is True
    assert sha(r['cmd'][-1]) == r['binary_sha256'] == o['binary_sha256']
    source = B / 'sources' / 'real' / ('tscwork.ts' if o['case'] == 'tscwork' else 'zodwork-census.ts')
    assert sha(source) == r['source_sha256']
    assert r['criterion'] == o['criterion']
    if o['case'] == 'tscwork':
        assert r['stdout'].count('ARMED_SELECTED_FULL') == 1
    label = r['label']
    raw = [json.loads(s) for s in (B / (label + '.jsonl')).read_text().splitlines()]
    assert raw == [o['census']]
    assert (B / 'logs' / (label + '.app.out')).read_text() == oracles[o['case']]
    c = o['census']
    tables = {t['table']: dict(bytes=t['bytes'], entries=t['entries'])
              for t in c['side_tables'] if t['table'] in
              ['arena.old_gen_page_objects', 'arena.old_gen_page_meta']}
    assert len(tables) == 2 and c['totals']['reachability_pass'] is True
    snapshots.append(dict(arm=o['arm'], case=o['case'], repeat=o['repeat'],
                          criterion=o['criterion'], tables=tables,
                          requested_bytes=sum(t['bytes'] for t in tables.values()),
                          gc_live_bytes=c['totals']['live_bytes'],
                          gc_live_objects=c['totals']['live_objects']))

medians = []
for case in ['tscwork', 'zodwork']:
    by_arm = {arm: statistics.median(s['requested_bytes'] for s in snapshots
              if s['arm'] == arm and s['case'] == case) for arm in [CONTROL, ARM]}
    medians.append(dict(case=case, requested_bytes=by_arm,
                        change_bytes=by_arm[ARM] - by_arm[CONTROL]))
inputs = {n: sha(B / n) for n in ['gc-pages-v5-census-runs.json',
          'gc-pages-v5-census-observations.json', 'gc-auto-v5-summary.json',
          'gc-pages-v5-noauto-summary.json', 'gc-pages-v5-linked-runs.json']}
save('gc-pages-v5-boundary-accounting.json', dict(
    scope='Two old-generation page tables; requested native storage, not allocator usable bytes or peak RSS.',
    limitations='TS boundary graphs differ by two live objects and 22,808 bytes; Zod uses explicit diagnostic collection, not the unmodified workload.',
    inputs=inputs, snapshots=snapshots, medians=medians))
save('gc-pages-v5-linux-validation.json', dict(
    scope='Isolated page compaction v5 on earlier production base; not current shared-visitor/window HEAD acceptance.',
    runtime_counts=counts, logs=logs, receipts=receipts, products=products,
    artifact_verification_sha256=sha(B / 'gc-runtime-pages-v5r2-artifact-verification.json'),
    linked=linked, inputs=inputs, normal_auto='204/204; comparison verifier passed'))
print(json.dumps(medians))
