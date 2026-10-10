from pathlib import Path
import json,os,subprocess
from statistics import median
L=Path('/root/lanes/perry-unpackcpu');D=L/'hello-control';D.mkdir(exist_ok=True)
rows=[]
for kind in ['direct','wrapped']:
 for n in range(5):
  for arm in (['base','fix'] if n%2==0 else ['fix','base']):
   stem=D/f'{kind}-{arm}-{n}';cmd=[str(L/'bins'/arm/'hello')]
   if kind=='wrapped':cmd=[str(L/'tools/time-exact'),str(stem.with_suffix('.time')),*cmd]
   r=subprocess.run(['taskset','-c','0-55','setarch','-R','perf','stat','-x',',','-e','instructions:u','-o',str(stem.with_suffix('.perf')),*cmd],capture_output=True)
   assert r.returncode==0 and r.stdout==b'hello\n'
   count=int(next(line.split(',')[0] for line in stem.with_suffix('.perf').read_text().splitlines() if ',instructions:u,' in line))
   rows.append(dict(kind=kind,n=n,arm=arm,instructions=count,parity=True))
for kind in ['direct','wrapped']:
 print(kind,{arm:[median(r['instructions'] for r in rows if r['kind']==kind and r['arm']==arm),min(r['instructions'] for r in rows if r['kind']==kind and r['arm']==arm),max(r['instructions'] for r in rows if r['kind']==kind and r['arm']==arm)] for arm in ['base','fix']})
(L/'evidence/hello-control.json').write_text(json.dumps(rows,indent=2)+'\n')
