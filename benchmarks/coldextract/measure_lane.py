from pathlib import Path
import subprocess,os,json,sys,shutil
L=Path('/root/lanes/perry-coldextract');N='/root/lanes/11842/node-v24.9.0-linux-x64/bin/node';O=L/'measure';kind=sys.argv[1];axis=sys.argv[2];arms=os.getenv('ARMS','base,fix,node').split(',');out=O/f'{os.getenv("ROW_PREFIX",kind)}-{axis}.jsonl'
manifest={r['name']:r for r in json.loads((L/'bins-base/manifest.json').read_text())}
def counts(path):
 d={}
 for line in path.read_text().splitlines():
  a=line.split(',')
  if len(a)>2:
   try:d[a[2]]=int(float(a[0]))
   except ValueError:pass
 return d
if kind=='micro':jobs=[(m,'micro',[m,'1' if m.startswith('write') else '5'],L/'micro') for m in ['noop','write-sync','write-async','write-part','hash','hash-small','file-hash','verify','verdict','parse-integrity','copies','views']]+[('spool','spool',['5'],L/'micro')]
elif kind=='upm':jobs=[(m,'upm',[],L) for m in os.getenv('UPM_MODES','cold,lock,offinst').split(',')]
else:jobs=[(r['name'],r['name'],r['args'],Path(r['cwd'])) for r in manifest.values() if r['name'] not in ['micro','spool','upm']]
for name,binary,args,cwd in jobs:
 if os.getenv('NAMES') and name not in os.environ['NAMES'].split(','):continue
 for i in range(5):
  outputs={};locks={}
  for arm in arms if i%2==0 else list(reversed(arms)):
   tag=f'{os.getenv("ROW_PREFIX",kind)}-{axis}-{name}-{arm}-{i}';stem=O/tag;env=os.environ.copy();env.pop('PERRY_GC_DIAG',None)
   cmd=[str(L/('bins-'+arm)/binary),*args] if arm!='node' else [N,str(L/'node-upm/dist/upm.mjs' if kind=='upm' else manifest[binary]['src']),*args]
   if axis=='gc':
    if arm!='node':env['PERRY_GC_DIAG']='1'
    else:cmd.insert(1,'--trace-gc')
   if axis=='thpoff' or os.getenv('THP_OFF')=='1':env['MIMALLOC_ALLOW_THP']='0';cmd=[str(L/'tools/no-thp'),*cmd]
   exact=stem.with_suffix('.exact');perf=stem.with_suffix('.perf');wrapped=[str(L/'tools/time-exact'),str(exact),*cmd]
   if axis!='gc':wrapped=['perf','stat','-x',',','-e','cycles:u' if axis=='cycles' else 'instructions:u','-o',str(perf),*wrapped]
   if kind=='upm':
    env['OBSERVE']='0';env['PRE']='' if axis=='gc' else 'perf stat -x , -e '+('cycles:u' if axis=='cycles' else 'instructions:u')+' -o '+str(perf)
    wrapped=[str(L/'r.sh'),tag,name,'1',str(L/'tools/time-exact'),str(exact),*cmd]
   with stem.with_suffix('.out').open('wb') as of,stem.with_suffix('.err').open('wb') as ef:
    p=subprocess.run(wrapped,cwd=cwd,env=env,stdout=of,stderr=ef,timeout=330)
   assert exact.exists(),(tag,p.returncode,stem.with_suffix('.err').read_text()[-500:]);d=exact.read_text().split()
   stdout=stem.with_suffix('.out').read_bytes();stderr=stem.with_suffix('.err').read_text(errors='replace')
   fulls=sum(x.startswith('[gc-full] site=') for x in stderr.splitlines()) if arm!='node' else sum(('Mark-Compact' in x or 'Mark-sweep' in x) for x in stdout.decode(errors='replace').splitlines())
   if axis=='gc' and arm=='node':stdout=b''.join(l for l in stdout.splitlines(keepends=True) if not (l.startswith(b'[') and b' ms:' in l and b'->' in l))
   row=dict(name=name,arm=arm,n=i,rc=p.returncode,wall=float(d[0]),user=float(d[1]),sys=float(d[2]),rss_kb=int(d[3]),fulls=fulls if axis=='gc' else None,**(counts(perf) if axis!='gc' else {}))
   if kind=='upm':
    run=L/'runs'/tag;data=(run/'res.tsv').read_text().split();row.update(rc=int(data[0]),fingerprint=data[5]);locks[arm]=json.loads((run/'1.lock').read_text());row['parity']=row['rc']==0 and data[5]=='ok'
    if axis=='gc':
     logs=(run/'1.log').read_text().splitlines();row['fulls']=sum(x.startswith('[gc-full] site=') for x in logs) if arm!='node' else sum(('Mark-Compact' in x or 'Mark-sweep' in x) for x in logs)
   else:
    outputs[arm]=stdout;row['parity']=p.returncode==0 and (kind=='micro' or stdout==(L/'bins-base'/(binary+'.node.out')).read_bytes());row['output']=stdout.decode(errors='replace').strip() if kind=='micro' else None
   with out.open('a') as f:f.write(json.dumps(row)+'\n')
   print(name,arm,i,row['rc'],round(row['wall'],5),row['parity'],flush=True);assert row['parity'],tag
   if kind=='micro' and (name.startswith('write') or name=='spool'):
    # Independently verify actual disk bytes and permissions after timing.
    from hashlib import sha512
    import base64
    rows=json.loads((L/'micro/files.json').read_text());rows=[r for r in rows if r['size']>=4*1024*1024] if name=='spool' else rows;expected={(r['integrity'],0o555 if r['exec'] else 0o444) for r in rows}
    for tree in (L/'work').glob('micro-*'):
     got=set()
     for path in tree.rglob('*'):
      if path.is_file():got.add(('sha512-'+base64.b64encode(sha512(path.read_bytes()).digest()).decode(),path.stat().st_mode&0o777))
     assert got==expected,(tag,'tree differs',len(got),len(expected));shutil.rmtree(tree)
  if kind=='upm':assert all(v==next(iter(locks.values())) for v in locks.values()),(name,i,'lock parity')
  else:assert all(v==next(iter(outputs.values())) for v in outputs.values()),(name,i,'output parity')
print('DONE',kind,axis,flush=True)
