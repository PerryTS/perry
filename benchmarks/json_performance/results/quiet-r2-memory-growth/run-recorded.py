from pathlib import Path
import hashlib,json,math,os,re,subprocess,sys,time
root=Path(__file__).resolve().parents[2]
worker=root/'.work/decoder-r2/rotating-worker'
out=Path(sys.argv[1]);out.mkdir(parents=True,exist_ok=False)
meta=dict(purpose='default-GC memory growth diagnostic; not CPU acceptance',
          worker_sha256=hashlib.sha256(worker.read_bytes()).hexdigest(),
          runner_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
          host=os.uname().nodename,started_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),
          counts=[1,8,16,32],warmup=0,mode='rotating',fixtures={})
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};env['PERRY_GC_DIAG']='1'
rows=[]
for fixture in ['wide_1m','records_object_20m']:
 prefix=root/'.work/rotating'/fixture
 meta['fixtures'][fixture]={str(i):hashlib.sha256(Path(str(prefix)+f'.{i}.json').read_bytes()).hexdigest() for i in range(8)}
 for count in meta['counts']:
  name=f'{fixture}-{count}'
  command=['/usr/bin/time','-l',str(worker),str(prefix),'parse',str(count),'0','time','rotating']
  result=subprocess.run(command,env=env,capture_output=True,check=True,timeout=120)
  (out/(name+'.stdout')).write_bytes(result.stdout);(out/(name+'.stderr')).write_bytes(result.stderr)
  fields=list(map(float,re.search(rb'^RESULT (.+)$',result.stdout,re.M).group(1).split()))
  assert len(fields)==7 and all(math.isfinite(n) for n in fields) and fields[-2:]==[count,0],name
  assert b'KEEP 8 1' in result.stdout,name
  diagnostics=result.stderr.decode(errors='replace')
  peak=int(re.search(r'(\d+)\s+maximum resident set size',diagnostics).group(1))
  def last(key):
   values=re.findall(r'\b'+key+r'=(\d+)',diagnostics)
   return int(values[-1]) if values else None
  row=dict(fixture=fixture,count=count,rss_before_mib=fields[3]/1048576,rss_after_mib=fields[4]/1048576,peak_rss_mib=peak/1048576,
           counters={k:last(k) for k in ['copying_minors','cycle_starts','completions','arena_live','arena_capacity','old_in_use','old_reclaimable','malloc','next_malloc','loop_polls']})
  rows.append(row)
  (out/'summary.json').write_text(json.dumps(rows,indent=2)+'\n')
  print(json.dumps(row),flush=True)
meta['finished_utc']=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
