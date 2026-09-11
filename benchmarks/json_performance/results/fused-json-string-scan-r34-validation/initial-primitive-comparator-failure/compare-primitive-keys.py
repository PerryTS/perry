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
for arm in ['main','candidate']:
 r=json.loads((w/(arm+'-primitive-keys-roots.json')).read_text())['rows'];assert len(r)==3 and all(c['exit_code']==0 for c in r)
(w/'primitive-keys-comparison.json').write_text(json.dumps(dict(candidate_all_pass=True,main_passes=4,candidate_passes=4,ir_matches=records,note='Fresh complete Node output checks for toJSON keys, callback order, reentrancy and BigInt. Native/shadow static checks pass. This fixture has no forced GC liveness claim; the separate expanded emitter fixture requires positive moving/protected collection.'),indent=2)+'\n')
print('Verified four primitive-key checks per arm and two matching IR files.')
