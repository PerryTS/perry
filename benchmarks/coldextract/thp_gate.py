from pathlib import Path
import subprocess,os,json,time
L=Path('/root/lanes/perry-coldextract')
while not (L/'final-gate.rc').exists():time.sleep(15)
assert (L/'final-gate.rc').read_text().strip()=='0'
while not (L/'post-profiles.rc').exists():time.sleep(15)
assert (L/'post-profiles.rc').read_text().strip()=='0'
subprocess.run(['python3',str(L/'summarize_lane.py')],stdout=(L/'summary.log').open('w'),check=True)
stats=json.loads((L/'summary/statistics.json').read_text());selection={}
for kind in ['micro','programs','upm']:
 names=[]
 for name,arms in stats[kind+'-inst'].items():
  b=arms['base']['rss_kb']['median'];f=arms['fix']['rss_kb']['median']
  if abs(f-b)>=1024:names.append(name)
 if kind in ['micro','upm']:
  for name,arms in stats[kind+'-cycles'].items():
   if abs(arms['fix']['rss_kb']['median']-arms['base']['rss_kb']['median'])>=1024 and name not in names:names.append(name)
 selection[kind]=names
 if not names:continue
 env=os.environ.copy();env.update(NAMES=','.join(names),ARMS='base,fix',ROW_PREFIX='thpoff-'+kind,THP_OFF='1')
 for axis in ['thpoff','gc']:
  with (L/('thpoff-'+kind+'-'+axis+'.log')).open('w') as f:
   subprocess.run(['taskset','-c','0-55','setarch','-R','python3',str(L/'measure_lane.py'),kind,axis],env=env,stdout=f,stderr=f,check=True)
  print(kind,axis,names,flush=True)
(L/'summary/thp-selection.json').write_text(json.dumps(selection,indent=2))
subprocess.run(['taskset','-c','0-55','setarch','-R','python3',str(L/'smaps_control.py')],check=True)
(L/'thp-gate.rc').write_text('0\n')
