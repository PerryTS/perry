from pathlib import Path
import hashlib,json,os,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
main='--main' in sys.argv;arm='main' if main else 'candidate';build=w/('frozen-main' if main else 'frozen-build')
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};records=[]
shared=w.with_name('materialized-read-r23')
sources=[('retained-zero', w.with_name('zero-spacing-r25')/'retained-zero-worker.ts')]
for name,source in sources:
 binary=w/(arm+'-'+name);obj=w/(arm+'-'+name+'.o')
 commands=[[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','--no-link','-o',str(obj)],['cc',str(obj),str(build/'libperry_runtime.a'),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)]]
 for i,cmd in enumerate(commands):
  with (w/(arm+'-'+name+'-build-'+str(i)+'.log')).open('wb') as log:subprocess.run(cmd,cwd=root,env=clean|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 records.append({'worker':name,'commands':commands,'files':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [source,obj,binary,build/'perry',build/'libperry_runtime.a',build/'libperry_stdlib.a']}})
 print('BUILT',arm,name,flush=True)
(w/(arm+'-retained-zero-workers-provenance.json')).write_text(json.dumps(records,indent=2)+'\n')
