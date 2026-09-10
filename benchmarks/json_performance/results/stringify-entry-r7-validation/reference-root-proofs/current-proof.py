from pathlib import Path
import os,subprocess,json
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
frozen=root/'benchmarks/json_performance/.work/native-main-proof/frozen-build'
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
passes=subprocess.check_output(['python3',str(root/'scripts/read_statepoint_rewrite_passes.py')],text=True).strip()
checks=[]
subjects=[('main-current-projection',frozen,root/'test-files/test_json_scalar_projection.ts'),('short-native',w/'frozen-build',w/'short-loops.ts')]
for label,build,source in subjects:
 d=w/label;d.mkdir(exist_ok=False)
 env=clean|{'PERRY_RUNTIME_DIR':str(build),'PERRY_WORKSPACE_ROOT':str(root),'PERRY_RS4GC':'1','PERRY_GC_MOVING_LOOP_POLLS':'1','PERRY_INLINE_SHADOW_SLOT':'0'}
 cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','--no-link','--trace','llvm','-o',str(d/'worker.o')]
 with (d/'compile.log').open('wb') as log:subprocess.run(cmd,cwd=d,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 paths=[]
 for i,ll in enumerate((d/'.perry-trace/llvm').glob('*.ll')):
  dest=d/('worker-'+str(i)+'.ll');paths.append(str(dest))
  with (d/('rewrite-'+str(i)+'.log')).open('wb') as log:subprocess.run(['/opt/homebrew/opt/llvm/bin/opt','-passes='+passes,'-S',str(ll),'-o',str(dest)],stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 with (d/'check.log').open('wb') as log:
  result=subprocess.run(['python3',str(root/'scripts/gc_root_dominance_check.py'),'--statepoints','--min-files','1','--min-statepoints','1','--min-live-bundles','1','--min-relocates','1']+paths,stdout=log,stderr=subprocess.STDOUT)
 checks.append({'subject':label,'exit_code':result.returncode,'compile_command':cmd})
 print(label,result.returncode,flush=True)
(w/'current-proof.json').write_text(json.dumps(checks,indent=2)+'\n')
