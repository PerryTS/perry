from pathlib import Path
import hashlib,json,os,re,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
source=w/'callback-only.ts';candidate='--candidate' in sys.argv;arm='candidate' if candidate else 'r5';build=w/'frozen-build' if candidate else w.parent/'invariant-field-loop-r5/frozen-build';binary=w/(arm+'-callback-only')
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','--trace','llvm','-o',str(binary)]
with (w/(arm+'-callback-compile.log')).open('wb') as log:subprocess.run(cmd,env=clean|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source)],env=clean,capture_output=True,timeout=180);assert node.returncode==0
knobs={'PERRY_GC_SCHEDULE_SEED':'10022','PERRY_GC_SCHEDULE_RATE':'0.1','PERRY_GC_SCHEDULE_ALLOC_KB':'0','PERRY_GC_PROTECT_FROMSPACE':'1','PERRY_GC_DIAG':'1'}
r=subprocess.run([str(binary)],env=clean|knobs,capture_output=True,timeout=180);(w/(arm+'-callback.stdout')).write_bytes(r.stdout);(w/(arm+'-callback.stderr')).write_bytes(r.stderr);assert r.returncode==0,(r.returncode,r.stderr[-2000:]);assert r.stdout==node.stdout,(r.stdout,node.stdout)
d=r.stderr.decode();protected=len(re.findall(r'\[gc-fromspace-protect\].*retired_set=#',d));moved=sum(sum(map(int,re.findall(r'\b(?:copied_objects|promoted_objects)=(\d+)',line))) for line in d.splitlines() if line.startswith('[gc-copy-minor] ran'));assert protected>0 and moved>0,(protected,moved)
meta={'arm':arm,'command':cmd,'matches_node':True,'protected_retired_sets':protected,'moved_objects':moved,'only_javascript_loop_is_inside_replacer':True,'files':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [source,binary,build/'perry',build/'libperry_runtime.a']}}
(w/(arm+'-callback-proof.json')).write_text(json.dumps(meta,indent=2)+'\n');print(arm,protected,moved,r.stdout.decode(),flush=True)
