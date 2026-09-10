from pathlib import Path
import hashlib,json,os,subprocess
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
sources=[root/'test-files'/n for n in ['test_gap_json_stringify_replacer_tojson.ts','test_gap_json_replacer_sparse_hole.ts','test_gap_json_prototype_tojson.ts','test_issue_9398_json_pretty_tombstoned_key.ts','test_gap_2021_json_stringify_grown_array.ts']]
records=[]
for source in sources:
 node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source)],env=clean,capture_output=True,timeout=180);assert node.returncode==0
 (w/(source.stem+'-node.stdout')).write_bytes(node.stdout)
 for arm,build in [('r5',w.parent/'invariant-field-loop-r5/frozen-build'),('candidate',w/'frozen-build')]:
  label=arm+'-'+source.stem;binary=w/label
  cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','-o',str(binary)]
  with (w/(label+'-compile.log')).open('wb') as log:subprocess.run(cmd,env=clean|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
  r=subprocess.run([str(binary)],env=clean,capture_output=True,timeout=180)
  (w/(label+'.stdout')).write_bytes(r.stdout);(w/(label+'.stderr')).write_bytes(r.stderr)
  records.append({'arm':arm,'source':str(source.relative_to(root)),'matches_node':r.stdout==node.stdout,'exit_code':r.returncode,'command':cmd,'hashes':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [source,binary,build/'perry',build/'libperry_runtime.a']}})
  print(label,r.returncode,r.stdout==node.stdout,flush=True)
(w/'existing-validation.json').write_text(json.dumps(records,indent=2)+'\n')
assert all(r['matches_node'] and r['exit_code']==0 for r in records),records
