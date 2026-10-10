from pathlib import Path
import json,subprocess,fcntl,os,time,re,shutil
L=Path('/root/lanes/perry-unpackcpu');D=L/'profiles-writes';D.mkdir(exist_ok=True)
while 'COMPLETE' not in (L/'validation-resume.log').read_text():time.sleep(15)
lock=open('/root/MEASURE.lock','w');fcntl.flock(lock,fcntl.LOCK_EX)
Path('/root/MEASURE.holder').write_text('perry-unpackcpu/write-trace\n')
kinds=['write','writev','pwrite64','pwritev','pwritev2'];events='syscalls:sys_exit_eventfd2,'+','.join('syscalls:sys_'+direction+'_'+kind for kind in kinds for direction in ['enter','exit'])
rows=[]
for arm in ['base','fix','node']:
 stem=D/arm;store=L/'work'/('write-trace-'+arm)
 if os.getenv('READ_EXISTING')!='1':
  shutil.rmtree(store,ignore_errors=True)
  stem.with_suffix('.data').unlink(missing_ok=True)
  cmd=[str(L/'bins'/arm/'micro')] if arm!='node' else ['/root/lanes/11842/node-v24.9.0-linux-x64/bin/node','--experimental-strip-types',str(L/'micro/micro.ts')]
  with stem.with_suffix('.out').open('w') as out,stem.with_suffix('.err').open('w') as err:
   r=subprocess.run(['taskset','-c','56-63','setarch','-R','perf','record','-q','-e',events,'-o',str(stem.with_suffix('.data')),'--',*cmd,'worker-stream','all','3',str(store)],stdout=out,stderr=err)
  assert r.returncode==0
  with stem.with_suffix('.samples').open('w') as f:subprocess.run(['perf','script','-i',str(stem.with_suffix('.data')),'-F','comm,tid,time,event,trace'],stdout=f,check=True)
 entered={};event_fds=set();latency=0;count=0;written=0;by_kind={}
 for line in stem.with_suffix('.samples').read_text().splitlines():
  m=re.match(r'^\s*(.*?)\s+(\d+)\s+([\d.]+):\s+syscalls:sys_(enter|exit)_(\w+):\s*(.*)$',line)
  if not m:continue
  comm,tid,stamp,direction,kind,fields=m.groups();stamp=float(stamp);key=(tid,kind)
  if kind=='eventfd2':
   returned=re.search(r'(0x[\da-fA-F]+|-?\d+)',fields)
   if returned:event_fds.add(int(returned[1],0))
   continue
  if direction=='enter':
   fd=re.search(r'\bfd:\s*(0x[\da-fA-F]+|-?\d+)',fields)
   if fd and int(fd[1],0)>2 and int(fd[1],0) not in event_fds:entered[key]=(stamp,comm)
  elif key in entered:
   start,who=entered.pop(key);elapsed=(stamp-start)*1000
   ret=re.search(r'\bret:\s*(0x[\da-fA-F]+|-?\d+)',fields)
   bare=re.match(r'\s*(0x[\da-fA-F]+|-?\d+)',fields)
   val=int((ret or bare)[1],0) if (ret or bare) else 0
   if 0<val<1<<63:
    latency+=elapsed;count+=1;written+=val
    bucket=by_kind.setdefault(kind,dict(calls=0,bytes=0,elapsed_ms=0));bucket['calls']+=1;bucket['bytes']+=val;bucket['elapsed_ms']+=elapsed
 assert count>100 and 0<=written-3*45_909_375<=1,(arm,count,written)
 row=dict(arm=arm,rounds=3,cpus='56-63',lock='/root/MEASURE.lock',tracepoint_elapsed_ms_per_round=latency/3,calls_per_round=count/3,bytes_per_round=written/3,auxiliary_write_bytes=written-3*45_909_375,logical_index_bytes_per_round=46_008_087,existing_per_part_duplicate_bytes_per_round=98_712,by_kind=by_kind,scope='successful fd>2 writes excluding recorded eventfds; elapsed includes kernel execution and waits; excludes metadata syscalls')
 rows.append(row);print(json.dumps(row),flush=True)
 (L/'evidence/write-traces.json').write_text(json.dumps(rows,indent=2)+'\n')
 stem.with_suffix('.data').unlink(missing_ok=True);shutil.rmtree(store,ignore_errors=True)
print('COMPLETE',flush=True)
