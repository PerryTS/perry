from pathlib import Path
import os,subprocess,json,sys,fcntl,time,hashlib,shutil,stat
L=Path('/root/lanes/perry-unpackcpu');N='/root/lanes/11842/node-v24.9.0-linux-x64/bin/node'
kind=sys.argv[1];arms=os.getenv('ARMS','base,fix,node').split(',');cycles=kind=='wall';gc=kind.endswith('gc');aff='56-63' if cycles else '0-55'
O=L/'measure';O.mkdir(exist_ok=True)
lock=open('/root/MEASURE.lock','w') if cycles else None
if lock:fcntl.flock(lock,fcntl.LOCK_EX);Path('/root/MEASURE.holder').write_text('perry-unpackcpu/micro '+time.strftime('%FT%TZ',time.gmtime())+'\n')
def tree(root):
 result={}
 for p in sorted(Path(root).rglob('*')):
  if p.is_file():result[str(p.relative_to(root))]=[stat.S_IMODE(p.stat().st_mode),hashlib.sha256(p.read_bytes()).hexdigest()]
 return result
cases=[(m,s,1 if m.startswith('worker') else 5) for s in ['large','all'] for m in ['noop','worker-noop','hash','inflate','parse','worker','worker-stream','sha256','sha1']]
if len(sys.argv)>2:cases=[c for c in cases if c[0] in sys.argv[2:]]
for mode,sel,rounds in cases:
 name=mode+'-'+sel
 for i in range(int(os.getenv('ROUNDS','5'))):
  for arm in arms if i%2==0 else arms[::-1]:
   tag=f'micro-{kind}-{name}-{arm}-{i}';stem=O/tag;cmd=[str(L/'bins'/('base' if arm=='repeat' else arm)/'micro')] if arm!='node' else [N,*(['--trace-gc'] if gc else []),'--experimental-strip-types',str(L/'micro/micro.ts')]
   cmd += [mode,sel,str(rounds),str(L/'work'/tag)]
   e=os.environ.copy();e.pop('PERRY_GC_DIAG',None)
   if gc and arm!='node':e['PERRY_GC_DIAG']='1'
   prefix=([str(L/'tools/no-thp')] if os.getenv('THP_OFF')=='1' else [])+['taskset','-c',aff,'setarch','-R','perf','stat','-x',',','-e','instructions:u,cycles:u,page-faults' if cycles else 'instructions:u,page-faults','-o',str(stem)+'.perf',str(L/'tools/time-exact'),str(stem)+'.time']
   with open(str(stem)+'.out','wb') as f,open(str(stem)+'.err','wb') as err:r=subprocess.run(prefix+cmd,cwd=L/'micro',env=e,stdout=f,stderr=err,timeout=180)
   raw=Path(str(stem)+'.out').read_bytes();out=b''.join(l for l in raw.splitlines(keepends=True) if not(l.startswith(b'[') and b' ms:' in l and b'->' in l))
   expected=O/(name+'.node.out')
   if not expected.exists():
    ref=subprocess.run([N,'--experimental-strip-types',str(L/'micro/micro.ts'),mode,sel,str(rounds),str(L/'work/node-oracle')],capture_output=True,timeout=180);assert ref.returncode==0;expected.write_bytes(ref.stdout)
    if mode.startswith('worker'):(O/(name+'.node.tree.json')).write_text(json.dumps(tree(L/'work/node-oracle'),sort_keys=True))
   parity=r.returncode==0 and out==expected.read_bytes()
   if mode.startswith('worker'):
    parity=parity and tree(L/'work'/tag)==json.loads((O/(name+'.node.tree.json')).read_text())
    shutil.rmtree(L/'work'/tag)
   wall,user,sy,rss=map(float,Path(str(stem)+'.time').read_text().split())
   c={l.split(',')[2]:int(l.split(',')[0]) for l in Path(str(stem)+'.perf').read_text().splitlines() if l.split(',')[0].isdigit()}
   logs=Path(str(stem)+'.err').read_text(errors='replace') if arm!='node' else raw.decode(errors='replace')
   fulls=(sum(l.startswith('[gc-full] site=') for l in logs.splitlines()) if arm!='node' else sum('Mark-Compact' in l or 'Mark-sweep' in l for l in logs.splitlines())) if gc else None
   row=dict(kind='micro-'+kind,name=name,arm=arm,n=i,wall=wall,user=user,sys=sy,rss_kb=int(rss),fulls=fulls,parity=parity,rc=r.returncode,cpus=aff,**c)
   with (O/('micro-'+kind+'.jsonl')).open('a') as f:f.write(json.dumps(row)+'\n')
   print(name,arm,i,r.returncode,round(wall,4),parity,c,flush=True);assert parity and c.get('instructions:u',0)>0
print('COMPLETE',kind,flush=True)
