from pathlib import Path
import os,subprocess,hashlib,json
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
source=w/'short-loops.ts';meta=[]
for label,base in [('main','native-main-proof'),('r3','scalar-projection-r3')]:
 frozen=root/'benchmarks/json_performance/.work'/base/'frozen-build'
 env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}|{'PERRY_RUNTIME_DIR':str(frozen)}
 obj=w/(label+'-short.o');binary=w/(label+'-short')
 commands=[[str(frozen/'perry'),'compile',str(source),'--no-cache','--no-auto-optimize','--no-link','-o',str(obj)],['cc',str(obj),str(frozen/'libperry_runtime.a'),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)]]
 for i,cmd in enumerate(commands):
  with (w/(label+'-short-build-'+str(i)+'.log')).open('wb') as log:
   subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 paths=[source,obj,binary]+[frozen/n for n in ['perry','libperry_runtime.a','libperry_stdlib.a']]
 meta.append({'label':label,'commands':commands,'hashes':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}})
 print('BUILT',label,flush=True)
(w/'short-reference-provenance.json').write_text(json.dumps(meta,indent=2)+'\n')
