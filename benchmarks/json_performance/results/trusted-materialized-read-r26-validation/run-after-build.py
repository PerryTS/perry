from pathlib import Path
import hashlib,json,re,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3]
build=json.loads((w/'build-provenance.json').read_text())
assert build['source_commit']==subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
for n,r in build['files'].items():assert hashlib.sha256((w/'frozen-build'/n).read_bytes()).hexdigest()==r['sha256']
for step in [['validate-zero.py','--candidate'],['check-zero-roots.py','--candidate'],['run-candidate-validation.py'],['build-changing-worker.py'],['check-changing-roots.py','--candidate'],['validate-changing-options.py','--candidate'],['build-retained-zero-worker.py'],['check-retained-roots.py','--candidate'],['validate-retained-zero.py','--candidate']]:
 subprocess.run(['python3',str(w/step[0]),*step[1:]],cwd=root,check=True)
assert (w/'main-changing-options.o').read_bytes()==(w/'candidate-changing-options.o').read_bytes()
assert (w/'main-retained-zero.o').read_bytes()==(w/'candidate-retained-zero.o').read_bytes()
records=[]
for mode in ['native','shadow']:
 a=w/('main-zero-ir-'+mode)/'zero.ll';b=w/('candidate-zero-ir-'+mode)/'zero.ll'
 x=a.read_bytes();y=b.read_bytes()
 if mode=='native':
  assert x.startswith(b'; ModuleID = ') and y.startswith(b'; ModuleID = ')
  x=x.split(b'\n',1)[1];y=y.split(b'\n',1)[1]
 assert x==y,mode
 records.append({'mode':mode,'normalized_sha256':hashlib.sha256(x).hexdigest(),'main_sha256':hashlib.sha256(a.read_bytes()).hexdigest(),'candidate_sha256':hashlib.sha256(b.read_bytes()).hexdigest()})
for arm in ['main','candidate']:
 checks=json.loads((w/(arm+'-zero-roots.json')).read_text())['rows']
 assert len(checks)==3 and all(r['exit_code']==0 for r in checks)
(w/'zero-root-comparison.json').write_text(json.dumps({'matches':True,'files':records},indent=2)+'\n')
additional=[]
for stem in ['changing','retained']:
 for mode in ['native','shadow']:
  a=w/('main-'+stem+'-ir-'+mode)/'zero.ll';b=w/('candidate-'+stem+'-ir-'+mode)/'zero.ll'
  x=a.read_bytes();y=b.read_bytes()
  if mode=='native':
   assert x.startswith(b'; ModuleID = ') and y.startswith(b'; ModuleID = ')
   x=x.split(b'\n',1)[1];y=y.split(b'\n',1)[1]
  assert x==y,(stem,mode)
  additional.append({'subject':stem,'mode':mode,'main_sha256':hashlib.sha256(a.read_bytes()).hexdigest(),'candidate_sha256':hashlib.sha256(b.read_bytes()).hexdigest(),'normalized_sha256':hashlib.sha256(x).hexdigest()})
 verdicts={arm:json.loads((w/(arm+'-'+stem+'-roots.json')).read_text())['rows'] for arm in ['main','candidate']}
 for rows in verdicts.values():
  assert len(rows)==3 and all(r['exit_code']==0 for r in rows if r['mode']=='shadow' or stem=='retained')
 assert [(r['mode'],r['variant'],r['exit_code']) for r in verdicts['main']]==[(r['mode'],r['variant'],r['exit_code']) for r in verdicts['candidate']]
 def fingerprints(arm):return sorted(re.findall(r'fingerprint\s*:\s*([^\n]+)',(w/(arm+'-'+stem+'-ir-native')/'check-0.log').read_text()))
 assert fingerprints('main')==fingerprints('candidate')
(w/'additional-root-comparison.json').write_text(json.dumps({'matches':True,'files':additional,'note':'Retained-zero native/shadow checks pass. Changing-record shadow checks pass; native retains four unsuppressed R25 unrooted/global property-write findings, with identical IR and fingerprints. Not a clean native verdict.'},indent=2)+'\n')
print('All R26 behavior/options/GC/IR and known-failure comparisons completed.',flush=True)
