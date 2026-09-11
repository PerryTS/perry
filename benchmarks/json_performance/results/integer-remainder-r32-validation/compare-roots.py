from pathlib import Path
import hashlib,json,re
w=Path(__file__).resolve().parent;records=[]
for mode in ['native','shadow']:
 a=w/('main-ir-'+mode);b=w/('candidate-ir-'+mode);left=sorted(p.name for p in a.glob('*.ll'));right=sorted(p.name for p in b.glob('*.ll'));assert left==right and len(left)==9
 for name in left:
  x=(a/name).read_bytes();y=(b/name).read_bytes();xx=x;yy=y
  if mode=='native':
   assert x.startswith(b'; ModuleID = ') and y.startswith(b'; ModuleID = ')
   xx=x.split(b'\n',1)[1];yy=y.split(b'\n',1)[1]
  same=xx==yy
  records.append({'mode':mode,'file':name,'same':same,'main_sha256':hashlib.sha256(x).hexdigest(),'candidate_sha256':hashlib.sha256(y).hexdigest(),'normalized_sha256':hashlib.sha256(xx).hexdigest(),'normalization':'only first ModuleID path comment removed' if mode=='native' else 'none'})
  assert same,(mode,name,'Inspect actual IR delta before claiming baseline root equivalence')
 if mode=='native':
  def fingerprints(path):return sorted(re.findall(r'fingerprint\s*:\s*([^\n]+)',path.read_text()))
  assert fingerprints(a/'check-0.log')==fingerprints(b/'check-0.log')
main=json.loads((w/'main-roots.json').read_text())['checks'];candidate=json.loads((w/'candidate-roots.json').read_text())['checks']
assert [(x['mode'],x['scope'],x['exit_code']) for x in main]==[(x['mode'],x['scope'],x['exit_code']) for x in candidate]
(w/'root-comparison.json').write_text(json.dumps({'ir_files':records,'checks_match':True,'native_fingerprints':fingerprints(w/'candidate-ir-native/check-0.log')},indent=2)+'\n');print('PASS all18IR files identical to actualmain after only native ModuleID path-comment normalization; full static verdicts and fingerprints agree')
