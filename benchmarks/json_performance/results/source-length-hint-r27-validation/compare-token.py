from pathlib import Path
import hashlib,json,re,subprocess
w=Path(__file__).resolve().parent
for arm in ['main','candidate']:
 data=json.loads((w/(arm+'-token-validation.json')).read_text());rows=data['rows']
 assert len(rows)==9 and all(r['exit_code']==0 and r['matches_node'] for r in rows)
 assert all(r['moved_objects']>0 and r['protected_retired_sets']>0 for r in rows if r['mode']=='scheduled')
records=[]
for mode in ['native','shadow']:
 a=w/('main-token-ir-'+mode)/'token.ll';b=w/('candidate-token-ir-'+mode)/'token.ll'
 x=a.read_bytes();y=b.read_bytes()
 if mode=='native':
  assert x.startswith(b'; ModuleID = ') and y.startswith(b'; ModuleID = ')
  x=x.split(b'\n',1)[1];y=y.split(b'\n',1)[1]
 assert x==y,mode
 records.append(dict(mode=mode,main_sha256=hashlib.sha256(a.read_bytes()).hexdigest(),candidate_sha256=hashlib.sha256(b.read_bytes()).hexdigest(),normalized_sha256=hashlib.sha256(x).hexdigest()))
rows={arm:json.loads((w/(arm+'-token-roots.json')).read_text())['rows'] for arm in ['main','candidate']}
for r in rows.values():
 assert len(r)==3 and all(x['exit_code']==0 for x in r if x['mode']=='shadow')
assert [(r['mode'],r['variant'],r['exit_code']) for r in rows['main']]==[(r['mode'],r['variant'],r['exit_code']) for r in rows['candidate']]
def fingerprints(arm):return sorted(re.findall(r'fingerprint\s*:\s*([^\n]+)',(w/(arm+'-token-ir-native')/'check-0.log').read_text()))
assert fingerprints('main')==fingerprints('candidate')
(w/'token-root-comparison.json').write_text(json.dumps(dict(matches=True,files=records,native_findings=fingerprints('main'),note='Both new token checker runs are fresh. Any native findings remain unsuppressed baseline findings, not a clean native pass.'),indent=2)+'\n')
print('Verified nine token cases per arm, positive copying/protection, two matching IR files and checker fingerprints.')
