from pathlib import Path
import hashlib, json, re

w = Path(__file__).resolve().parent
root = w.parents[3]
sha = lambda b: hashlib.sha256(b).hexdigest()
source = root / 'test-files/test_json_large_string_emitters.ts'
records = {a: json.loads((w / (a + '-large-emitter-validation.json')).read_text()) for a in ['main', 'candidate']}
for arm, record in records.items():
    assert len(record['rows']) == 20
    for p, h in record['files'].items():
        assert sha((root / p).read_bytes()) == h, p
    assert all(r['protected_retired_sets'] > 0 and r['moved_objects'] > 0 for r in record['rows'] if r['mode'].startswith('scheduled'))
assert all(r['exit_code'] == 0 and r['matches_node'] for r in records['candidate']['rows'])
failures = [r for r in records['main']['rows'] if r['exit_code'] or not r['matches_node']]
ir = []
checks = {}
for arm in ['main', 'candidate']:
    proof = json.loads((w / (arm + '-large-emitter-roots.json')).read_text())
    assert proof['source_sha256'] == sha(source.read_bytes())
    rows = proof['rows']
    assert [(r['mode'], r['variant'], r['exit_code']) for r in rows] == [('native', 0, 1), ('shadow', 0, 0), ('shadow', 1, 0)]
    report = (w / (arm + '-large-emitter-ir-native/check-0.log')).read_text()
    fingerprints = re.findall(r'fingerprint\s*:\s*([^\n]+)', report)
    assert fingerprints == ['large-emitter.ll::perry_fn_test_json_large_string_emitters_ts__run::unrooted:strhandle->js_put_value_set_ic_overflow_store']
    assert 'MOVING           : no' in report and '(unrooted: 1, stale: 0)' in report
    checks[arm] = dict(native_report_sha256=sha(report.encode()),fingerprints=fingerprints)
assert checks['main'] == checks['candidate']
for mode in ['native', 'shadow']:
    paths = [w / (a + '-large-emitter-ir-' + mode) / 'large-emitter.ll' for a in ['main', 'candidate']]
    raw = [p.read_bytes() for p in paths]
    normalized = [b.split(b'\n', 1)[1] if mode == 'native' else b for b in raw]
    if mode == 'native':
        assert all(b.startswith(b'; ModuleID = ') for b in raw)
    assert normalized[0] == normalized[1], mode
    ir.append(dict(mode=mode, main_sha256=sha(raw[0]), candidate_sha256=sha(raw[1]), normalized_sha256=sha(normalized[0])))
result = dict(candidate_passes=20, main_passes=20-len(failures), main_failures=failures, ir_matches=ir, static_findings=checks,
              checker_sha256=sha((root/'scripts/gc_root_dominance_check.py').read_bytes()),
              note='Candidate executions are fresh; exact frozen R26 reference receipts are reused with source/artifact hash checks. Candidate outputs must match complete Node output, with positive moving/protected witnesses. The fresh native checker has one unsuppressed non-moving string-handle/overflow-store finding in both arms, with identical IR and full reports. Both shadow variants pass; no checker or allowlist was changed.')
(w/'large-emitter-comparison.json').write_text(json.dumps(result,indent=2)+'\n')
print('Verified 20 fresh candidate large-emitter checks and two equal IR files; native finding remains explicit in both arms.')
