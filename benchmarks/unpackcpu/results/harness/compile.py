from pathlib import Path
import os,subprocess,json,hashlib,sys,shutil
L=Path('/root/lanes/perry-unpackcpu'); R=L/'perry'; T=L/'target'; D=L/'drivers'; arm=sys.argv[1] if len(sys.argv)>1 else 'base'; B=L/'bins'/arm; B.mkdir(exist_ok=True,parents=True)
env=os.environ.copy(); env.pop('PERRY_NO_AUTO_OPTIMIZE',None);env.update(PATH=str(L/'build-tools')+':/root/.cargo/bin:'+env['PATH'],PERRY_LANE_AUTO_TARGET=str(L/('auto-target-'+arm)),CARGO_BUILD_JOBS='8',CARGO_TARGET_DIR=str(T),PERRY_RUNTIME_DIR=str(T/'release'),PERRY_WORKSPACE_ROOT=str(R),PERRY_CACHE_DIR=str(L/('cache-'+arm)),PERRY_KEEP_SYMBOLS='1',PERRY_ALLOW_PERRY_FEATURES='1')
programs=[("micro", L/"micro/micro.ts", ["hash","large","5"], L/"micro"), ('upm',Path('/root/lanes/upm-prof/upm/src/upm.ts'),[],Path('/root/lanes/upm-prof/upm')),('hello',D/'hello.ts',[],D),('tsc',D/'tscwork.ts',['3'],D),('zod',D/'zodwork.ts',['5000'],D),('qs_parse',D/'pk/qs/parse_nested.ts',['5000','500'],D/'pk'),('qs_stringify',D/'pk/qs/stringify_nested.ts',['5000','500'],D/'pk'),('commander',D/'pk/commander/parse_argv.ts',['5000','500'],D/'pk'),('fastify',D/'pk/fastify/inject.ts',['1000','100'],D/'pk'),('effect',D/'effect/main.ts',[],D/'effect'),('buffer_heavy',D/'buffer_heavy.ts',[],D),('worker_heavy',D/'worker_heavy.ts',[],D)]
rows=[]
for name,src,args,cwd in programs:
 if len(sys.argv)>2 and name not in sys.argv[2:]:continue
 e=env.copy()
 if name=='tsc':
  if arm=='base':
   default_log=B/'tsc-default.compile.log'
   with default_log.open('w') as f:default_rc=subprocess.run(['taskset','-c','0-55',str(T/'release/perry'),'compile',str(src),'--no-cache','-o',str(B/'tsc-default')],cwd=cwd,env=e,stdout=f,stderr=f).returncode
   (L/'tsc-default.rc').write_text(str(default_rc))
   if default_rc!=0:assert 'budget' in default_log.read_text().lower(),'default tsc failure was not a budget failure'
  if (L/'tsc-default.rc').read_text().strip()!='0':e['PERRY_LL_RS4GC_MAX_INSTRS']='2097152'

 log=B/(name+'.compile.log')
 with log.open('w') as f:rc=subprocess.run(['taskset','-c','0-55',str(T/'release/perry'),'compile',str(src),'--no-cache','-o',str(B/name)],cwd=cwd,env=e,stdout=f,stderr=f).returncode
 txt=log.read_text(errors='replace');valid=rc==0 and 'falling back' not in txt.lower() and 'auto-optimize build failed' not in txt.lower()
 row=dict(name=name,src=str(src),args=args,cwd=str(cwd),binary=str(B/name),compile_rc=rc,auto_optimized=valid)
 if valid:
  row['sha256']=hashlib.sha256((B/name).read_bytes()).hexdigest()
  if name!='upm':
   n=subprocess.run(['taskset','-c','0-55','/root/lanes/11842/node-v24.9.0-linux-x64/bin/node','--experimental-strip-types',str(src),*args],cwd=cwd,capture_output=True,timeout=300)
   p=subprocess.run(['taskset','-c','0-55',str(B/name),*args],cwd=cwd,capture_output=True,timeout=300)
   for suffix,data in [('node.out',n.stdout),('node.err',n.stderr),('out',p.stdout),('err',p.stderr)]: (B/(name+'.'+suffix)).write_bytes(data)
   row.update(parity=n.returncode==p.returncode==0 and n.stdout==p.stdout,node_rc=n.returncode,perry_rc=p.returncode)
 if arm=='fix' and name=='micro' and valid:
  with (L/'early-fixed-budget.log').open('w') as f:probe=subprocess.run(['python3',str(L/'backend_budget.py'),str(B/name),str(src),'--node','/root/lanes/11842/node-v24.9.0-linux-x64/bin/node','--output',str(L/'validation/early-fixed')],stdout=f,stderr=f)
  assert probe.returncode==0,'fixed LLVM-linked backend did not pass instruction witness'
  with (L/'early-inflate-budget.log').open('w') as f:inflate=subprocess.run(['python3',str(L/'backend_budget.py'),str(B/name),str(src),'--stage','inflate','--node','/root/lanes/11842/node-v24.9.0-linux-x64/bin/node','--output',str(L/'validation/early-inflate')],stdout=f,stderr=f)
  assert inflate.returncode==0,'shared LLVM-linked inflater did not pass instruction witness'
 rows.append(row);(B/'manifest.json').write_text(json.dumps(rows,indent=2)+'\n');print(name,rc,valid,row.get('parity'),flush=True)
 if not valid:sys.exit(1)
 # Keep only the primary target and the current auto-optimized target.
 for auto in (R/'target').glob('perry-auto-*'):
  if auto.is_dir():shutil.rmtree(auto)
 shutil.rmtree(L/('cache-'+arm),ignore_errors=True)
# Auto targets contain only build outputs; binaries retain the selected, source-checked archives.
shutil.rmtree(L/('auto-target-'+arm),ignore_errors=True)
