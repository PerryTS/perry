from pathlib import Path
import json,hashlib
w=Path(__file__).resolve().parent
records=[]
for arm in ['main','candidate']:
 rows=json.loads((w/(arm+'-remainder-validation.json')).read_text())['rows']
 assert len(rows)==6 and all(r['exit_code']==0 and r['matches_node'] for r in rows)
 assert all(r['moved_objects']>0 and r['protected_retired_sets']>0 for r in rows if r['mode']=='scheduled')
 checks=json.loads((w/(arm+'-remainder-roots.json')).read_text())['rows']
 assert [(r['mode'],r['variant'],r['exit_code']) for r in checks]==[('native',0,0),('shadow',0,1),('shadow',1,0)]
 report=(w/(arm+'-remainder-ir-shadow')/'check-0.log').read_text()
 assert report.count('fingerprint:')==1 and 'fingerprint: remainder.ll::main::js_array_alloc_literal->js_array_mark_numeric_f64_layout' in report
 assert '=== violations: 1   (moving-minor reachable: 0)' in report

for mode in ['native','shadow']:
 a=w/('main-remainder-ir-'+mode)/'remainder.ll';b=w/('candidate-remainder-ir-'+mode)/'remainder.ll';x=a.read_bytes();y=b.read_bytes()
 if mode=='native':
  assert x.startswith(b'; ModuleID = ') and y.startswith(b'; ModuleID = ');x=x.split(b'\n',1)[1];y=y.split(b'\n',1)[1]
 assert x==y
 records.append(dict(mode=mode,main_sha256=hashlib.sha256(a.read_bytes()).hexdigest(),candidate_sha256=hashlib.sha256(b.read_bytes()).hexdigest(),normalized_sha256=hashlib.sha256(x).hexdigest()))
(w/'remainder-comparison.json').write_text(json.dumps(dict(candidate_passes=6,reference_passes=6,ir_matches=records,note='Fresh complete-output Node parity and positive moving/protected GC in native/shadow, both arms. Native and unrooted-alloca checks pass. Shadow checker retains one unsuppressed numeric-array initialization violation, identical fingerprint and IR in reference/candidate. Not a clean shadow verdict. Raw numeric-runtime optimization; no JSON/GC source change.'),indent=2)+'\n')
print('Six remainder checks per arm, positive copying/protection and two matching IR files pass.')
