"""Verify full private normal-auto results; retain original harness failures.

The reused generic driver labels document builds by workload mode, while the
strict source-to-build validator expects the source stem (70-documents). This
observer preserves the original manifest and any terminal failure, and creates
a separately attested binding manifest with both labels. It never reruns or
excludes measurements and accepts only the specific final bookkeeping failure.
"""
import pathlib, json, hashlib, importlib.util, re, subprocess
B = pathlib.Path('/root/rss-header-20261002')
V = 'gc-private-currentc170-v25-auto'
E = B / 'primary-gc-runtime-v24/export'
def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def load(n): return json.loads((B / (V + '-' + n + '.json')).read_text())
receipt = B / (V + '-driver.exit')
assert receipt.exists(), 'original driver has not terminated; do not restart it'
rc = receipt.read_text().strip()
assert rc in ['0', '1']
if rc == '1':
    log = (B / (V + '-driver.log')).read_text()
    assert 'm.validate_builds(rows,builds)' in log
    assert log.rstrip().endswith('ValueError: measurement has no matching successful build for its source: 70-documents-replace')
spec = importlib.util.spec_from_file_location('summary', B / 'summarize-gc-comparison.py')
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
r, b, o, products = [load(n) for n in ['runs', 'builds', 'oracle', 'products']]
assert len(r) == 204 and len(b) == 34 and len(o) == 17
assert all(sha(p) == h for p, h in products.items())
bound = []
for row in b:
    assert row['rc'] == 0 and row['reason'] is None
    assert sha(row['cmd'][2]) == row['source_sha256']
    assert sha(row['cmd'][0]) == row['compiler_sha256']
    assert sha(E / row['arm'] / 'libperry_runtime.a') == row['runtime_sha256']
    assert sha(row['cmd'][-1]) == row['binary_sha256']
    assert (B / 'logs' / (row['label'] + '.out')).read_text() == row['stdout']
    source_case = pathlib.Path(row['cmd'][2]).stem
    assert source_case == row['case'] or row['case'].startswith(source_case + '-')
    bound.append(dict(row, case=source_case, workload_case=row['case']))
for row in r:
    assert (B / 'logs' / (row['label'] + '.out')).read_text() == row['stdout'] == o[row['case']]
    err = (B / 'logs' / (row['label'] + '.err')).read_text()
    if row['mode'] == 'plain':
        assert int(re.search(r'RSS_KIB=(\d+)', err)[1]) * 1024 == row['peak_rss_bytes']
    else:
        events = {f[2]: int(f[0]) for line in err.splitlines() if len(f := line.split(',')) > 2 and f[2] in ['instructions', 'cycles', 'minor-faults', 'major-faults']}
        assert events == row['events']
m.validate(r, o, ['control', 'elf'], m.AUTO_CASES, True)
m.validate_builds(r, bound)
derived = B / (V + '-source-build-bindings.json')
assert not derived.exists(), 'independent observer already produced a manifest'
derived.write_text(json.dumps(bound, indent=2) + '\n')
summary = dict(scope='Complete private GNU x86-64 root-scope probe vs current GC proposal; moving/root acceptance separate.',
               complete_cells=204, cases=m.summarize(r, ['control', 'elf'], m.AUTO_CASES, True),
               original_driver_exit=rc, original_builds_sha256=sha(B/(V+'-builds.json')),
               source_build_bindings_sha256=sha(derived))
(B/(V+'-independent-summary.json')).write_text(json.dumps(summary, indent=2)+'\n')
proof = dict(complete_cells=204, complete_cases=17, all_correct=True,
             source_binary_products_stdout_rss_counters_summary_verified=True,
             original_driver_exit=rc, source_build_binding_derivation='Only case normalized to actual source stem; original workload_case preserved. Original builds/runs/logs/receipt unchanged.',
             inputs={n: sha(B/(V+'-'+n+'.json')) for n in ['runs','builds','oracle','products','source-build-bindings','independent-summary']},
             build_verification_sha256=sha(E/'independent-build-verification.json'),
             limitations='Three repeats on shared host. Auto-optimize archives differ per workload; full wrapper hashes attest starting products, while workspace input proofs and application hashes bind specialized builds. Production adoption and moving/root checks separate.')
(B/(V+'-verification.json')).write_text(json.dumps(proof, indent=2)+'\n')
print(json.dumps(proof, indent=2))
