from pathlib import Path
import hashlib,json,os,re,subprocess,sys
work=Path(__file__).resolve().parent
root=work.parents[3]
mode=sys.argv[1]
assert mode in ['r2','main']
build=(work.parent/'scalar-projection-r2/frozen-build') if mode=='r2' else work/'frozen-build'
source=work/'test_json_scalar_projection.ts'
out=work/mode; out.mkdir(exist_ok=False)
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
env|={'PERRY_RUNTIME_DIR':str(build),'PERRY_WORKSPACE_ROOT':str(root),
      'PERRY_RS4GC':'1','PERRY_INLINE_SHADOW_SLOT':'0','PERRY_GC_MOVING_LOOP_POLLS':'1','PERRY_NO_AUTO_OPTIMIZE':'1'}
cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','--no-link','--trace','llvm','-o',str(out/'fixture.o')]
with (out/'compile.log').open('wb') as log:
 subprocess.run(cmd,cwd=out,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
files=list((out/'.perry-trace/llvm').glob('*.ll')); assert files
counts=0; rewritten=[]
for i,path in enumerate(files):
 counts+=len(re.findall(r'\bcall\s+double\s+@js_json_lazy_index_scalar\(',path.read_text()))
 dest=out/f'fixture-{i}.ll'
 subprocess.run(['/opt/homebrew/opt/llvm/bin/opt','-passes='+ (work/'passes.txt').read_text().strip(),'-S',str(path),'-o',str(dest)],check=True,timeout=180)
 rewritten.append(str(dest))
assert (counts>0) if mode=='r2' else (counts==0)
with (out/'checker.log').open('wb') as log:
 result=subprocess.run(['python3',str(work/'checker.py'),'--statepoints','--min-files','1','--min-statepoints','1','--min-live-bundles','1','--min-relocates','1']+rewritten,stdout=log,stderr=subprocess.STDOUT)
paths=[source,work/'checker.py',work/'passes.txt']+[build/n for n in ['perry','libperry_runtime.a','libperry_stdlib.a']]
meta=dict(mode=mode,commands=[cmd],scalar_probe_calls=counts,checker_exit_code=result.returncode,files={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths})
(out/'result.json').write_text(json.dumps(meta,indent=2)+'\n')
print(mode,'probes',counts,'checker',result.returncode,flush=True)
print((out/'checker.log').read_text(),flush=True)
