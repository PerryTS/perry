"""External RSS/mapping diagnosis; no debugger, injected GC or workload edits.

Captures smaps on RSS growth and separately obtains exact process max RSS from
wait4. Mapping reads are not atomic; sampling distance from exact high-water is
reported explicitly. These observations do not replace acceptance measurements.
"""
import pathlib,json,hashlib,subprocess,os,time,re
B=pathlib.Path('/root/rss-header-20261002');Q=B/'qb6-currentc170-build-v22';V='gc-currentc170-peak-mappings-v28';O=B/V;O.mkdir(exist_ok=False)
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
builds=json.loads((B/'gc-currentc170-qb6-v22-noauto-builds.json').read_text())
oracles=json.loads((B/'gc-currentc170-qb6-v22-noauto-oracle.json').read_text())
def maps(text,binary):
 rows=[];cur=None
 for line in text.splitlines():
  if re.match(r'^[0-9a-f]+-[0-9a-f]+ ',line):
   fields=line.split(maxsplit=5);cur=dict(range=fields[0],permissions=fields[1],path=fields[5] if len(fields)>5 else '',values={});rows.append(cur)
  elif cur and ':' in line:
   k,v=line.split(':',1);n=v.strip().split()
   if len(n)==2 and n[1]=='kB':cur['values'][k]=int(n[0])*1024
 totals={};rss=anon=0
 for row in rows:
  r=row['values']['Rss'];a=row['values']['Anonymous'];assert 0<=a<=r
  rss+=r;anon+=a;path=row['path'];perm=row['permissions']
  category='program executable pages' if path==str(binary) and 'x' in perm else 'program other file-backed pages' if path==str(binary) else 'other file-backed pages'
  totals['anonymous pages']=totals.get('anonymous pages',0)+a
  totals[category]=totals.get(category,0)+r-a
 assert sum(totals.values())==rss
 return dict(rss_bytes=rss,anonymous_bytes=anon,categories=totals,mappings=rows)
records=[]
for rep in range(3):
 for case,args in [('tscwork',['1']),('zodwork',['200'])]:
  for arm in (['currentc170-base','currentc170-gc'] if rep%2==0 else ['currentc170-gc','currentc170-base']):
   binary=Q/'bin'/f'gc-qb6-currentc170-v22-noauto-{arm}-{case}';build=next(x for x in builds if x['arm']==arm and x['case']==case)
   assert sha(binary)==build['binary_sha256']
   prefix=f'{V}-{arm}-{case}-{rep}';out=O/(prefix+'.out');err=O/(prefix+'.err');best=None;last_rss=-1;samples=0;started=time.monotonic()
   env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}|{'MIMALLOC_ALLOW_THP':'0'}
   with out.open('w') as of,err.open('w') as ef:
    p=subprocess.Popen([str(binary),*args],cwd=B,env=env,stdin=subprocess.DEVNULL,stdout=of,stderr=ef)
    proc=pathlib.Path('/proc')/str(p.pid)
    while True:
     pid,status,usage=os.wait4(p.pid,os.WNOHANG)
     if pid:p.returncode=os.waitstatus_to_exitcode(status);break
     try:
      assert os.readlink(proc/'exe')==str(binary)
      before=(proc/'status').read_text();rss=int(re.search(r'^VmRSS:\s+(\d+) kB$',before,re.M)[1])*1024
      if rss>last_rss+256*1024:
       raw=(proc/'smaps').read_text();after=(proc/'status').read_text();mapping=maps(raw,binary);samples+=1;last_rss=rss
       if best is None or mapping['rss_bytes']>best['rss_bytes']:
        best=dict(mapping,seconds=time.monotonic()-started,status_before=before,status_after=after)
        (O/(prefix+'.smaps')).write_text(raw)
     except (FileNotFoundError,ProcessLookupError,AttributeError):pass
     assert time.monotonic()-started<300,'owned diagnostic exceeded runtime bound'
     time.sleep(.005)
   assert p.returncode==0 and out.read_text()==oracles[case] and best is not None
   maximum=int(usage.ru_maxrss)*1024
   # Concurrent mapping reads can straddle state changes; preserve signed gap.
   record=dict(arm=arm,case=case,repeat=rep,pid=p.pid,rc=p.returncode,correct=True,binary_sha256=sha(binary),source_sha256=build['source_sha256'],exact_wait4_max_rss_bytes=maximum,sampled_mapping_rss_bytes=best['rss_bytes'],signed_peak_gap_bytes=maximum-best['rss_bytes'],samples=samples,seconds=time.monotonic()-started,categories=best['categories'],anonymous_bytes=best['anonymous_bytes'],capture_seconds=best['seconds'],smaps_sha256=sha(O/(prefix+'.smaps')),stdout_sha256=sha(out),stderr_sha256=sha(err))
   (O/(prefix+'.snapshot.json')).write_text(json.dumps(best,indent=2)+'\n');records.append(record);(B/(V+'-runs.json')).write_text(json.dumps(records,indent=2)+'\n')
   print(arm,case,rep,'peak',maximum,'sample',best['rss_bytes'],'gap',record['signed_peak_gap_bytes'],flush=True)
assert len(records)==12
(B/(V+'.exit')).write_text('0\n')
