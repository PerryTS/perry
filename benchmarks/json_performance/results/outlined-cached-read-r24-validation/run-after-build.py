from pathlib import Path
import hashlib,json,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3]
record=json.loads((w/'build-provenance.json').read_text())
assert record['source_commit']==subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
for n,r in record['files'].items():assert hashlib.sha256((w/'frozen-build'/n).read_bytes()).hexdigest()==r['sha256']
for name in ['run-candidate-validation.py','inspect-disassembly.py','compare-json-entry-disassembly.py']:
 subprocess.run(['python3',str(w/name)],cwd=root,check=True)
print('R24 candidate validation and disassembly complete; stage and timing remain separate.',flush=True)
