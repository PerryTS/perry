from pathlib import Path
import hashlib,json,os,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};passes=subprocess.check_output(['python3',str(root/'scripts/read_statepoint_rewrite_passes.py')],cwd=root,text=True).strip()
subjects=[('main-entry',w.parent/'native-main-proof/frozen-build',root/'test-files/test_json_stringify_entry.ts'),('r5-options',w.parent/'invariant-field-loop-r5/frozen-build',w/'options-worker.ts'),('r5-callback',w.parent/'invariant-field-loop-r5/frozen-build',w/'callback-only.ts')]
if '--candidate' in sys.argv:subjects=[('candidate-options',w/'frozen-build',w/'options-worker.ts'),('candidate-callback',w/'frozen-build',w/'callback-only.ts')]
records=[]
for label,build,source in subjects:
 d=w/('ir-'+label);d.mkdir(exist_ok=False);env=clean|{'PERRY_RUNTIME_DIR':str(build),'PERRY_WORKSPACE_ROOT':str(root),'PERRY_RS4GC':'1','PERRY_GC_MOVING_LOOP_POLLS':'1','PERRY_INLINE_SHADOW_SLOT':'0'}
 cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','--no-link','--trace','llvm','-o',str(d/'worker.o')]
 with (d/'compile.log').open('wb') as log:subprocess.run(cmd,cwd=d,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 files=list((d/'.perry-trace/llvm').glob('*.ll'));assert files
 outputs=[]
 for i,p in enumerate(files):
  dest=d/('worker-'+str(i)+'.ll');outputs.append(str(dest))
  with (d/('rewrite-'+str(i)+'.log')).open('wb') as log:subprocess.run(['/opt/homebrew/opt/llvm/bin/opt','-passes='+passes,'-S',str(p),'-o',str(dest)],stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 with (d/'check.log').open('wb') as log:r=subprocess.run(['python3',str(root/'scripts/gc_root_dominance_check.py'),'--statepoints','--max-stale','0','--min-files','1','--min-statepoints','1','--min-live-bundles','1','--min-relocates','1']+outputs,stdout=log,stderr=subprocess.STDOUT,cwd=root)
 records.append({'subject':label,'exit_code':r.returncode,'command':cmd,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'compiler_sha256':hashlib.sha256((build/'perry').read_bytes()).hexdigest(),'runtime_sha256':hashlib.sha256((build/'libperry_runtime.a').read_bytes()).hexdigest()});print(label,r.returncode,flush=True)
(w/('candidate-extra-ir.json' if '--candidate' in sys.argv else 'reference-extra-ir.json')).write_text(json.dumps(records,indent=2)+'\n')
