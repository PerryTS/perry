import gzip,json,os,sys
from pathlib import Path
R=Path(os.environ.get('PERRY_BIRTH_OUTPUT', '.'));mapping={}
name=sys.argv[1] if len(sys.argv)>1 else 'base'
for l in open(R/(name+'.dump'),errors='replace'):
 if not l.startswith('D {'):continue
 if '"record":"birth-injection"' in l:break
 if '"record":"birth-edge"' in l:
  r=json.loads(l[2:])
  if r.get('unit')!='<census>':mapping[r['target']]=r['source']
with gzip.open(R/'instance-origins.json.gz','wt') as f:json.dump(mapping,f)
print('summary slot source mappings',len(mapping),flush=True)
