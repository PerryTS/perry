from pathlib import Path
import subprocess,shutil,os,time,json,hashlib
L=Path('/root/lanes/perry-coldextract');R=L/'perry';S=L/'base-worktree'
# Keep original main's Git build stamp as well as its source contents.
if not S.exists():subprocess.run(['git','worktree','add','--quiet','--detach',str(S),'origin/main'],cwd=R,check=True)
source=(L/'micro/unpack.ts').read_text();start=source.index('interface Spool {');end=source.index('/** A packed file',start)
tail=source.index('let token = "";');tailend=source.index('/** Paths a package',tail)
header='import { builtin } from "./builtin.ts";\nimport { pid } from "./runtime.ts";\nimport { createWriter } from "./unpack.ts";\nconst CHUNK = 4 * 1024 * 1024;\nconst STREAM_FILE = CHUNK;\n'
(L/'micro/spool.ts').write_text(header+source[start:end]+source[tail:tailend]+(L/'spool_harness.ts').read_text())
while not (L/'target-base/release/perry').exists():time.sleep(15)
T=L/'target-base';env=os.environ.copy();env.pop('PERRY_NO_AUTO_OPTIMIZE',None);env.update(PATH='/root/.cargo/bin:'+env['PATH'],CARGO_TARGET_DIR=str(T),CARGO_BUILD_JOBS='8',PERRY_RUNTIME_DIR=str(T/'release'),PERRY_WORKSPACE_ROOT=str(S),PERRY_CACHE_DIR=str(L/'cache-base-spool'),PERRY_KEEP_SYMBOLS='1',PERRY_ALLOW_PERRY_FEATURES='1')
with (L/'bins-base/spool.compile.log').open('w') as f:p=subprocess.run(['taskset','-c','0-55',str(T/'release/perry'),'compile',str(L/'micro/spool.ts'),'--no-cache','-o',str(L/'bins-base/spool')],cwd=L/'micro',env=env,stdout=f,stderr=f)
text=(L/'bins-base/spool.compile.log').read_text();assert p.returncode==0 and 'falling back' not in text.lower() and 'auto-optimize build failed' not in text.lower()
outputs=[]
for arm,cmd in [('node',['/root/lanes/11842/node-v24.9.0-linux-x64/bin/node',str(L/'micro/spool.ts'),'5']),('base',[str(L/'bins-base/spool'),'5'])]:
 p=subprocess.run(['taskset','-c','0-55',*cmd],cwd=L/'micro',capture_output=True,env=env);assert p.returncode==0,(arm,p.stderr);outputs.append(p.stdout);(L/'bins-base'/('spool.'+arm+'.out')).write_bytes(p.stdout)
 for tree in (L/'work').glob('micro-*'):shutil.rmtree(tree)
assert outputs[0]==outputs[1]
row=dict(name='spool',src=str(L/'micro/spool.ts'),args=['5'],cwd=str(L/'micro'),binary=str(L/'bins-base/spool'),auto_optimized=True,parity=True,sha256=hashlib.sha256((L/'bins-base/spool').read_bytes()).hexdigest())
p=L/'bins-base/manifest.json';rows=json.loads(p.read_text());rows.append(row);p.write_text(json.dumps(rows,indent=2))
print('spool baseline compiled from original-main worktree, default auto optimization, Node parity',flush=True)
(L/'spool-base.rc').write_text('0\n')
