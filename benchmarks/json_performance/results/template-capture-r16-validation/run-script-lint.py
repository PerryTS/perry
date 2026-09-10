from pathlib import Path
import hashlib,json,os,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3];record=json.loads((w/'unit-source.json').read_text());record.pop('env',None);record.pop('exit_code',None);record['command']=['env','SKIP_COMPILE_GATES=1','./scripts/run_lint_gates.sh'];record['hashes']['scripts/shape_descriptor_census_baseline.json']=hashlib.sha256((root/'scripts/shape_descriptor_census_baseline.json').read_bytes()).hexdigest()
assert record['source_commit']==subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
with (w/'script-lint.log').open('wb') as log:r=subprocess.run(['./scripts/run_lint_gates.sh'],cwd=root,env=clean|{'SKIP_COMPILE_GATES':'1'},stdout=log,stderr=subprocess.STDOUT)
record['exit_code']=r.returncode;(w/'script-lint-source.json').write_text(json.dumps(record,indent=2)+'\n')
with (w/'file-cap.log').open('wb') as log:cap=subprocess.run(['./scripts/check_file_size.sh'],cwd=root,stdout=log,stderr=subprocess.STDOUT)
print('Lint terminal',r.returncode,'file cap terminal',cap.returncode,flush=True)
raise SystemExit(r.returncode or cap.returncode)
