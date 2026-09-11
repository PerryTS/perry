from pathlib import Path
import json,hashlib
w=Path(__file__).resolve().parent;rows={a:json.loads((w/(a+'-emitter-validation.json')).read_text())['rows'] for a in ['main','candidate']}
assert all(len(v)==20 for v in rows.values())
assert all(r['exit_code']==0 and r['matches_node'] for r in rows['candidate'])
assert all(r['protected_retired_sets']>0 and r['moved_objects']>0 for v in rows.values() for r in v if r['mode'].startswith('scheduled'))
failures=[r for r in rows['main'] if r['exit_code']!=0 or not r['matches_node']]
assert len(failures)==6 and all(r['mode'].startswith('scheduled') and r['parser'] in ['auto','direct'] for r in failures)
records=[]
for mode in ['native','shadow']:
 a=w/('main-emitter-ir-'+mode)/'emitter.ll';b=w/('candidate-emitter-ir-'+mode)/'emitter.ll'
 x=a.read_bytes();y=b.read_bytes()
 if mode=='native':
  assert x.startswith(b'; ModuleID = ') and y.startswith(b'; ModuleID = ')
  x=x.split(b'\n',1)[1];y=y.split(b'\n',1)[1]
 assert x==y
 records.append(dict(mode=mode,main_sha256=hashlib.sha256(a.read_bytes()).hexdigest(),candidate_sha256=hashlib.sha256(b.read_bytes()).hexdigest(),normalized_sha256=hashlib.sha256(x).hexdigest()))
for arm in ['main','candidate']:
 r=json.loads((w/(arm+'-emitter-roots.json')).read_text())['rows'];assert len(r)==3 and all(c['exit_code']==0 for c in r)
(w/'emitter-comparison.json').write_text(json.dumps(dict(candidate_all_pass=True,main_passes=14,main_existing_failures=6,candidate_passes=20,ir_matches=records,note='Both arms freshly executed at the same real test-files path. Six reference scheduled failures are fixed, with positive copying/protected sets; normal/full-GC/tape controls preserve complete Node output. Fresh native/shadow static checks pass in both arms.'),indent=2)+'\n')
print('Verified 20 candidate emitter checks, six repaired reference failures, real copying/protection and two matching IR files.')
