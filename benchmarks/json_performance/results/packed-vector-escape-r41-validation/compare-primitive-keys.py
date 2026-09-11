from pathlib import Path
import json,hashlib
w=Path(__file__).resolve().parent;rows={a:json.loads((w/(a+'-primitive-keys-validation.json')).read_text())['rows'] for a in ['main','candidate']}
assert all(len(v)==4 for v in rows.values())
assert all(r['exit_code']==0 and r['matches_node'] for r in rows['candidate'])
assert all(r['exit_code']==0 and r['matches_node'] for r in rows['main'])
records=[]
for mode in ['native','shadow']:
 a=w/('main-primitive-keys-ir-'+mode)/'primitive-keys.ll';b=w/('candidate-primitive-keys-ir-'+mode)/'primitive-keys.ll'
 x=a.read_bytes();y=b.read_bytes()
 if mode=='native':
  assert x.startswith(b'; ModuleID = ') and y.startswith(b'; ModuleID = ')
  x=x.split(b'\n',1)[1];y=y.split(b'\n',1)[1]
 assert x==y
 records.append(dict(mode=mode,main_sha256=hashlib.sha256(a.read_bytes()).hexdigest(),candidate_sha256=hashlib.sha256(b.read_bytes()).hexdigest(),normalized_sha256=hashlib.sha256(x).hexdigest()))
root_findings=[]
for arm in ['main','candidate']:
 r=json.loads((w/(arm+'-primitive-keys-roots.json')).read_text())['rows']
 assert len(r)==3 and [(c['mode'],c['variant'],c['exit_code']) for c in r]==[('native',0,1),('shadow',0,0),('shadow',1,0)]
 report=(w/(arm+'-primitive-keys-ir-native')/'check-0.log').read_bytes()
 assert report==(w/'main-primitive-keys-ir-native/check-0.log').read_bytes()
 assert b'statepoint hazards: 1  (unrooted: 1, stale: 0)' in report and b'MOVING           : no' in report
 fingerprint='primitive-keys.ll::perry_closure_test_json_primitive_tojson_keys_ts__2::unrooted:global->js_array_push_f64_spec'
 assert report.count(b'fingerprint      : ')==1 and fingerprint.encode() in report
 root_findings.append(dict(arm=arm,checker_exit=1,fingerprint=fingerprint,moving=False,report_sha256=hashlib.sha256(report).hexdigest()))
(w/'primitive-keys-comparison.json').write_text(json.dumps(dict(candidate_all_pass=True,main_passes=4,candidate_passes=4,ir_matches=records,unsuppressed_native_root_findings=root_findings,note='Fresh complete Node output checks for toJSON keys, callback order, reentrancy and BigInt. Shadow static checks pass. Native checker fails with one byte-identical non-moving global-load finding in both arms; IR also matches. No checker allowance or source exemption was changed. This fixture has no forced GC liveness claim; the separate expanded emitter fixture requires positive moving/protected collection.'),indent=2)+'\n')
print('Verified four primitive-key checks per arm and two matching IR files.')
