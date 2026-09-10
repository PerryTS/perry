from pathlib import Path
import hashlib,json,os,subprocess
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent;build=w/'frozen-build'
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};records=[]
for stem in ['empty-replacer','empty-value-pretty']:
 source=w/(stem+'.ts');binary=w/('candidate-'+stem)
 cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','-o',str(binary)]
 with (w/('candidate-'+stem+'-compile.log')).open('wb') as log:subprocess.run(cmd,env=clean|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 r=subprocess.run([str(binary)],env=clean,capture_output=True,timeout=180);assert r.returncode==0
 (w/('candidate-'+stem+'.stdout')).write_bytes(r.stdout);(w/('candidate-'+stem+'.stderr')).write_bytes(r.stderr)
 suffix='-auto' if stem=='empty-replacer' else ''
 assert r.stdout==(w/('main-'+stem+suffix+'.stdout')).read_bytes()==(w/('r5-'+stem+suffix+'.stdout')).read_bytes()
 node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source)],env=clean,capture_output=True,timeout=180);assert node.returncode==0 and node.stdout!=r.stdout
 records.append({'probe':stem,'matches_main_and_r5':True,'matches_node':False,'command':cmd,'hashes':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [source,binary,build/'perry',build/'libperry_runtime.a']}})
 print('INHERITED GAP',stem,r.stdout.decode(),flush=True)
(w/'candidate-inherited-gaps.json').write_text(json.dumps(records,indent=2)+'\n')
