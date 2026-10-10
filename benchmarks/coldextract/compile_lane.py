from pathlib import Path
import subprocess,os,json,sys,hashlib,shutil
from concurrent.futures import ThreadPoolExecutor,as_completed
L=Path('/root/lanes/perry-coldextract');R=L/'perry';T=L/'target';D=L/'drivers';arm=sys.argv[1];B=L/('bins-'+arm)
env=os.environ.copy();env.pop('PERRY_NO_AUTO_OPTIMIZE',None);env.update(PATH='/root/.cargo/bin:'+env['PATH'],CARGO_BUILD_JOBS='8',CARGO_TARGET_DIR=str(T),PERRY_RUNTIME_DIR=str(T/'release'),PERRY_WORKSPACE_ROOT=str(R),PERRY_CACHE_DIR=str(L/('cache-'+arm)),PERRY_KEEP_SYMBOLS='1',PERRY_ALLOW_PERRY_FEATURES='1')
if arm=='fix':env['CARGO_BUILD_JOBS']='2' # Two compiles plus validation's four jobs fit the lane cap.
jobs=[('micro',L/'micro/driver.ts',['hash','1'],L/'micro'),('upm',Path('/root/lanes/upm-prof/upm/src/upm.ts'),[],Path('/root/lanes/upm-prof/upm')),('hello',D/'hello.ts',[],D),('tsc',D/'tscwork.ts',['3'],D),('zod',D/'zodwork.ts',['5000'],D),('qs_parse',D/'pk/qs/parse_nested.ts',['5000','500'],D/'pk'),('qs_stringify',D/'pk/qs/stringify_nested.ts',['5000','500'],D/'pk'),('commander',D/'pk/commander/parse_argv.ts',['5000','500'],D/'pk'),('fastify',D/'pk/fastify/inject.ts',['1000','100'],D/'pk'),('effect',D/'effect/main.ts',[],D/'effect'),('buffer_heavy',D/'buffer_heavy.ts',[],D),('worker_heavy',D/'worker_heavy.ts',[],D)]
if arm=='fix':jobs.insert(1,('spool',L/'micro/spool.ts',['5'],L/'micro'))
rows=json.loads((B/'manifest.json').read_text()) if (B/'manifest.json').exists() else []
def compile_job(job):
 name,src,args,cwd=job
 job_env=env.copy()
 if name=='tsc':job_env['PERRY_LL_RS4GC_MAX_INSTRS']='2097152'
 with (B/(name+'.compile.log')).open('w') as f:p=subprocess.run(['taskset','-c','0-55',str(T/'release/perry'),'compile',str(src),'--no-cache','-o',str(B/name)],cwd=cwd,env=job_env,stdout=f,stderr=f)
 text=(B/(name+'.compile.log')).read_text(errors='replace');assert p.returncode==0 and 'falling back' not in text.lower() and 'auto-optimize build failed' not in text.lower(),name
 row=dict(name=name,src=str(src),args=args,cwd=str(cwd),binary=str(B/name),auto_optimized=True,sha256=hashlib.sha256((B/name).read_bytes()).hexdigest())
 if name!='upm':
  outs=[]
  for a,cmd in [('node',['/root/lanes/11842/node-v24.9.0-linux-x64/bin/node',str(src),*args]),(arm,[str(B/name),*args])]:
   p=subprocess.Popen(['taskset','-c','0-55',*cmd],cwd=cwd,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE);stdout,stderr=p.communicate(timeout=300);assert p.returncode==0,(name,a,stderr[-500:]);outs.append(stdout)
   (B/(name+'.'+a+'.out')).write_bytes(stdout);(B/(name+'.'+a+'.err')).write_bytes(stderr)
   if name=='spool':shutil.rmtree(L/'work'/('micro-spool-'+str(p.pid)))
  assert outs[0]==outs[1],(name,'parity');row['parity']=True
 return row
pending=[job for job in jobs if not any(row['name']==job[0] for row in rows)]
with ThreadPoolExecutor(max_workers=2 if arm=='fix' else 1) as pool:
 for future in as_completed([pool.submit(compile_job,job) for job in pending]):
  row=future.result();rows.append(row);(B/'manifest.json').write_text(json.dumps(rows,indent=2));print(row['name'],'compiled, auto optimized, parity',row.get('parity'),flush=True)
print('COMPLETE',arm,flush=True)
