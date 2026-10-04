"""Independent raw-log and product verification of private GC experiments."""
import pathlib, json, hashlib, importlib.util, re
B = pathlib.Path('/root/rss-header-20261002')
V = 'gc-private-currentc170-idle-eden-v30r1-noauto'
E = B / 'primary-idle-eden-v30/export'
exports = {'control': B / 'primary-gc-runtime-v24/export/control', 'idle': E}
def sha(p):
    with pathlib.Path(p).open('rb') as f: return hashlib.file_digest(f, 'sha256').hexdigest()
def load(n): return json.loads((B / (V + '-' + n + '.json')).read_text())
assert (B / (V + '.exit')).read_text().strip() == '0'
assert (B / 'gc-private-currentc170-idle-eden-v30r1-apps-driver.exit').read_text().strip() == '0'
spec = importlib.util.spec_from_file_location('summary', B / 'summarize-gc-comparison.py')
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
r, b, o, s, products = [load(n) for n in ['runs', 'builds', 'oracle', 'summary', 'products']]
assert len(r) == 48 and len(b) == 8 and len(o) == 4
assert all(sha(p) == h for p, h in products.items())
for row in b:
    assert sha(row['cmd'][2]) == row['source_sha256']
    assert sha(row['cmd'][0]) == row['compiler_sha256']
    assert sha(exports[row['arm']] / 'libperry_runtime.a') == row['runtime_sha256']
    assert sha(row['cmd'][-1]) == row['binary_sha256']
    assert (B / 'logs' / (row['label'] + '.out')).read_text() == row['stdout']
for row in r:
    assert (B / 'logs' / (row['label'] + '.out')).read_text() == row['stdout'] == o[row['case']]
    err = (B / 'logs' / (row['label'] + '.err')).read_text()
    if row['mode'] == 'plain':
        assert int(re.search(r'RSS_KIB=(\d+)', err)[1]) * 1024 == row['peak_rss_bytes']
    else:
        events = {f[2]: int(f[0]) for line in err.splitlines() if len(f := line.split(',')) > 2 and f[2] in ['instructions', 'cycles', 'minor-faults', 'major-faults']}
        assert events == row['events']
for a in ['idle']:
    pair = [x for x in r if x['arm'] in ['control', a]]
    m.validate(pair, o, ['control', a], tuple(o), True)
    assert s['comparisons'][a] == m.summarize(r, ['control', a], tuple(o), True)
m.validate_builds(r, b)
assert s['inputs'] == {n: sha(B / (V + '-' + n + '.json')) for n in ['runs', 'builds', 'oracle', 'products']}
proof = dict(complete_cells=48, source_binary_products_stdout_rss_counters_summary_verified=True,
             inputs={n: sha(B / (V + '-' + n + '.json')) for n in ['runs', 'builds', 'oracle', 'products', 'summary']},
             build_verification_sha256=sha(E / 'independent-build-verification.json'),
             limitations='Private four-case full-runtime comparison, three repeats on a shared host. No full normal-auto acceptance; probes unapplied.')
(B / (V + '-verification.json')).write_text(json.dumps(proof, indent=2) + '\n')
print(json.dumps(proof, indent=2))
