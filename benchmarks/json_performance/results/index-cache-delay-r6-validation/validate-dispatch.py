from pathlib import Path
import hashlib,json,os,subprocess
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
sources=[w/'dispatch-probe.ts',root/'test-files/test_gap_7574_array_subclass_declared_base_type.ts',root/'test-files/test_gap_typedarray_param_read.ts']
records=[]
for source in sources:
 label=source.stem
 oracle=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source)],capture_output=True,env=clean,timeout=180)
 (w/(label+'-node.stdout')).write_bytes(oracle.stdout);(w/(label+'-node.stderr')).write_bytes(oracle.stderr)
 assert oracle.returncode==0,(label,oracle.stderr)
 outputs={}
 for arm,frozen in [('r5',w.parent/'invariant-field-loop-r5/frozen-build'),('r6',w/'frozen-build')]:
  dest=w/(arm+'-'+label);cmd=[str(frozen/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','-o',str(dest)]
  with (w/(arm+'-'+label+'-compile.log')).open('wb') as log:
   subprocess.run(cmd,env=clean|{'PERRY_RUNTIME_DIR':str(frozen)},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
  r=subprocess.run([str(dest)],capture_output=True,env=clean,timeout=180)
  (w/(arm+'-'+label+'.stdout')).write_bytes(r.stdout);(w/(arm+'-'+label+'.stderr')).write_bytes(r.stderr)
  outputs[arm]=r.stdout
  records.append({'source':str(source.relative_to(root)),'arm':arm,'exit_code':r.returncode,'matches_node':r.stdout==oracle.stdout,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'binary_sha256':hashlib.sha256(dest.read_bytes()).hexdigest(),'compiler_sha256':hashlib.sha256((frozen/'perry').read_bytes()).hexdigest(),'runtime_sha256':hashlib.sha256((frozen/'libperry_runtime.a').read_bytes()).hexdigest(),'command':cmd})
  print(arm,label,r.returncode,r.stdout==oracle.stdout,flush=True)
  assert r.returncode==0,(arm,label,r.stderr)
 assert outputs['r6']==outputs['r5'],(label,outputs)
(w/'dispatch-validation.json').write_text(json.dumps(records,indent=2)+'\n')
assert all(r['matches_node'] for r in records), 'Node mismatch retained in dispatch-validation.json'
