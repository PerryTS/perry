from pathlib import Path
import hashlib,json,subprocess,os
w=Path(__file__).resolve().parent;root=w.parents[3];build=w/'frozen-main';source=w/'lazy-dispatch-probe.ts';binary=w/'main-lazy-dispatch-probe';clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','-o',str(binary)]
with (w/'main-lazy-dispatch-build.log').open('wb') as log:subprocess.run(cmd,cwd=root,env=clean|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True)
rows=[]
for shape in ['root','object','array']:
 for mode in ['plain','zero','true','pretty','callback','keys']:
  node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source),shape,mode],env=clean,capture_output=True,timeout=60);assert node.returncode==0,node.stderr
  r=subprocess.run([str(binary),shape,mode],env=clean,capture_output=True,timeout=60)
  label='lazy-dispatch-'+shape+'-'+mode
  for suffix,raw in [('node.stdout',node.stdout),('main.stdout',r.stdout),('main.stderr',r.stderr)]: (w/(label+'.'+suffix)).write_bytes(raw)
  row={'shape':shape,'mode':mode,'exit_code':r.returncode,'matches_node':r.stdout==node.stdout};rows.append(row);print(row,flush=True)
(w/'lazy-dispatch-main.json').write_text(json.dumps({'diagnostic_only':True,'source_commit':json.loads((w/'main-build-provenance.json').read_text())['source_commit'],'command':cmd,'files':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [source,binary,build/'perry',build/'libperry_runtime.a']},'rows':rows},indent=2)+'\n')
