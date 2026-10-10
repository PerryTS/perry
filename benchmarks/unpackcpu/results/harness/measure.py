from pathlib import Path
import os,subprocess,json,sys,time
L=Path('/root/lanes/perry-unpackcpu'); N='/root/lanes/11842/node-v24.9.0-linux-x64/bin/node'; kind=sys.argv[1]; O=L/'measure'; O.mkdir(exist_ok=True)
gc=kind.endswith('-gc'); upm=kind.startswith('upm'); cycles=kind=='upm-wall'; affinity='56-63' if cycles else '0-55'; repeats=int(os.getenv('ROUNDS','7' if cycles else '5')); arms=os.getenv('ARMS','base,node').split(','); names=sys.argv[2:]
if cycles:Path('/root/MEASURE.holder').write_text('perry-unpackcpu/'+kind+' '+time.strftime('%FT%TZ',time.gmtime())+'\n')
specs={r['name']:r for r in json.loads((L/'bins/base/manifest.json').read_text())}
def counters(p):
 d={}
 for l in p.read_text().splitlines():
  a=l.split(',')
  if len(a)>2 and a[0].strip().isdigit():d[a[2]]=int(a[0])
 return d
for name in names or (['cold','warm','lock','offinst'] if upm else [n for n in specs if n!='upm']):
 for i in range(repeats):
  locks=[]
  for arm in arms if i%2==0 else arms[::-1]:
   binary_arm='base' if arm=='repeat' else arm;tag=f'{kind}-{name}-{arm}-{i}';stem=O/tag;e=os.environ.copy();e.pop('PERRY_GC_DIAG',None)
   events='instructions:u,cycles:u,page-faults' if cycles else 'instructions:u,page-faults'
   prefix=([str(L/'tools/no-thp')] if os.getenv('THP_OFF')=='1' else [])+['taskset','-c',affinity,'setarch','-R','perf','stat','-x',',','-e',events,'-o',str(stem.with_suffix('.perf'))]
   if upm:
    e.update(OBSERVE='0',XENV=('UPM_REGISTRY=http://127.0.0.1:18771' if name in ('cold','lock') else '')+(' PERRY_GC_DIAG=1' if gc and arm!='node' else ''),PRE=' '.join(prefix))
    cmd=[str(L/'bins'/binary_arm/'upm')] if arm!='node' else [N,*(['--trace-gc'] if gc else []),str(L/'node-upm/dist/upm.mjs')]
    with stem.with_suffix('.runner.log').open('w') as f:rc=subprocess.run([str(L/'r.sh'),tag,name,'1',*cmd],env=e,stdout=f,stderr=f,timeout=330).returncode
    a=(L/'runs'/tag/'res.tsv').read_text().split();wall,user,sy,rss=map(float,a[1:5]); parity=a[0]=='0' and a[-1]=='ok';logs=(L/'runs'/tag/'1.log').read_text(errors='replace');locks.append(json.loads((L/'runs'/tag/'1.lock').read_text()))
   else:
    r=specs[name];cmd=[str(L/'bins'/binary_arm/name),*r['args']] if arm!='node' else [N,*(['--trace-gc'] if gc else []),'--experimental-strip-types',r['src'],*r['args']]
    if gc and arm!='node':e['PERRY_GC_DIAG']='1'
    with stem.with_suffix('.out').open('wb') as f,stem.with_suffix('.err').open('wb') as err:rc=subprocess.run([*prefix,str(L/'tools/time-exact'),str(stem.with_suffix('.time')),*cmd],env=e,cwd=r['cwd'],stdout=f,stderr=err,timeout=330).returncode
    wall,user,sy,rss=map(float,stem.with_suffix('.time').read_text().split());output=stem.with_suffix('.out').read_bytes();logs=stem.with_suffix('.err' if arm!='node' else '.out').read_text(errors='replace')
    if gc and arm=='node':output=b''.join(l for l in output.splitlines(keepends=True) if not (l.startswith(b'[') and b' ms:' in l and b'->' in l))
    parity=rc==0 and output==(L/'bins/base'/(name+'.node.out')).read_bytes()
   fulls=(sum(l.startswith('[gc-full] site=') for l in logs.splitlines()) if arm!='node' else sum('Mark-Compact' in l or 'Mark-sweep' in l for l in logs.splitlines())) if gc else None
   row=dict(kind=kind,name=name,arm=arm,n=i,rc=rc,wall=wall,user=user,sys=sy,rss_kb=int(rss),fulls=fulls,parity=parity,cpus=affinity,**counters(stem.with_suffix('.perf')))
   with (O/(kind+'.jsonl')).open('a') as f:f.write(json.dumps(row)+'\n')
   print(name,arm,i,rc,round(wall,4),parity,flush=True);assert parity and row.get('instructions:u',0)>0
  if upm:assert all(l==locks[0] for l in locks),'lock mismatch';(O/(f'{kind}-{name}-{i}.parity')).write_text('true\n')
print('COMPLETE',kind,flush=True)
