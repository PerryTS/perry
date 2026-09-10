from pathlib import Path
import hashlib,json,os,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
sources=[w/'options-worker.ts'];references=[('main',w.parent/'native-main-proof/frozen-build'),('r5',w.parent/'invariant-field-loop-r5/frozen-build')]
if '--candidate' in sys.argv: references=[('candidate',w/'frozen-build')]
records=[];env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
for label,build in references:
 source=sources[0];obj=w/(label+'-options.o');binary=w/(label+'-options')
 commands=[[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','--no-link','--trace','llvm','-o',str(obj)],['cc',str(obj),str(build/'libperry_runtime.a'),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)]]
 for i,cmd in enumerate(commands):
  with (w/(label+'-options-build-'+str(i)+'.log')).open('wb') as log:subprocess.run(cmd,env=env|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 paths=[source,obj,binary]+[build/n for n in ['perry','libperry_runtime.a','libperry_stdlib.a']]
 records.append({'label':label,'commands':commands,'files':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}})
 print('BUILT',label,flush=True)
(w/('candidate-options-provenance.json' if '--candidate' in sys.argv else 'reference-options-provenance.json')).write_text(json.dumps(records,indent=2)+'\n')
