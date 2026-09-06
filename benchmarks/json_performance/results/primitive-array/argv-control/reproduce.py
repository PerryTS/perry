from pathlib import Path
import subprocess,os,json,re,shutil,hashlib,statistics
root=Path.cwd();out=root/'results/argv-control';out.mkdir()
variants=[]
for arm,original,letter in [('unicode','.work/baseline-worker','u'),('primitive','.work/worker','p')]:
 for length in [6,15]:
  dest=root/'.work'/(letter*length);shutil.copy2(original,dest)
  variants.append((arm,length,dest))
records=[]
for rep in range(7):
 for arm,length,worker in variants[rep%4:]+variants[:rep%4]:
  env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
  p=subprocess.run(['/usr/bin/time','-l',str(worker),str(root/'.work/fixtures/numbers_1m.json'),'parse','96','2'],env=env,capture_output=True,text=True,timeout=60)
  assert p.returncode==0,p.stderr
  result=re.search(r'^RESULT (.+)$',p.stdout,re.M).group(1).split()
  assert float(result[5])==98 and float(result[6])==0,result
  row={'arm':arm,'executable_name_length':length,'worker':str(worker),'sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'rep':rep,'cpu_us':(float(result[1])+float(result[2]))/96,'rss_before':float(result[3]),'rss_after':float(result[4]),'peak_rss':int(re.search(r'(\d+)\s+maximum resident set size',p.stderr).group(1)),'stdout':p.stdout,'stderr':p.stderr}
  records.append(row)
  with (out/'trials.jsonl').open('a') as f:f.write(json.dumps(row)+'\n')
for arm,length,_ in variants:
 rows=[r for r in records if r['arm']==arm and r['executable_name_length']==length]
 print(arm,length,'CPU us',statistics.median(r['cpu_us'] for r in rows),'peak MiB',statistics.median(r['peak_rss'] for r in rows)/1048576,flush=True)
