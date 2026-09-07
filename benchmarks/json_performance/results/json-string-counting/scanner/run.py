from pathlib import Path
import subprocess,json
p=Path(__file__).resolve().parent
fixtures=Path.home()/'json-deferred-gc-v2-20260907-codex/.work/fixtures'
rows=[]
for fixture in ['escaped_1m','unicode_1m','records_object_1m']:
 for rep in range(5):
  for arm in ['before','after','after','before']:
   result=subprocess.run(['/usr/bin/time','-l',str(p/'probe'),str(fixtures/(fixture+'.json')),'400',arm],text=True,capture_output=True,check=True)
   fields=result.stdout.split();row=dict(fixture=fixture,rep=rep,arm=arm,wall_ns=int(fields[1]),checksum=int(fields[2]),stderr=result.stderr)
   rows.append(row)
 (p/'results/raw.json').write_text(json.dumps(rows,indent=2)+'\n')
 print(fixture,flush=True)
