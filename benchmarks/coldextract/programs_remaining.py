from pathlib import Path
import json
L=Path('/root/lanes/perry-coldextract');p=L/'measure/programs-inst.jsonl'
rows=[json.loads(x) for x in p.read_text().splitlines()] if p.exists() else []
names=[r['name'] for r in json.loads((L/'bins-base/manifest.json').read_text()) if r['name'] not in ['micro','spool','upm']]
remaining=[]
for name in names:
 group=[r for r in rows if r['name']==name]
 if not group:remaining.append(name);continue
 assert all(r['rc']==0 and r['parity'] for r in group),name
 assert all(len([r for r in group if r['arm']==a])==5 for a in ['base','fix','node']),name
print(','.join(remaining))
