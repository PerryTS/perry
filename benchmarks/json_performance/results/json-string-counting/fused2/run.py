from pathlib import Path
import subprocess,json,random
p=Path(__file__).resolve().parent
fixtures=Path.home()/'json-deferred-gc-v2-20260907-codex/.work/fixtures'
valid=(fixtures/'unicode_1m.json').read_bytes()
cases={f:fixtures/(f+'.json') for f in ['unicode_1m','escaped_1m','records_object_1m']}
for name,data in [('mostly-ascii', b'a'*400000+'é'.encode()+b'a'*400000),('invalid-first',b'\xff'+valid),('lone-surrogate',b'\xed\xa0\x80'+valid),('invalid-middle',valid[:len(valid)//2]+b'\xff'+valid[len(valid)//2:])]:
 q=p/(name+'.bin');q.write_bytes(data);cases[name]=q
rows=[];rng=random.Random(53017)
for fixture,path in cases.items():
 for rep in range(5):
  order=['compat','basic','fused'];rng.shuffle(order)
  for arm in order+order[::-1]:
   result=subprocess.run(['/usr/bin/time','-l',str(p/'probe'),str(path),'1000',arm],text=True,capture_output=True,check=True)
   fields=result.stdout.split();rows.append(dict(fixture=fixture,rep=rep,arm=arm,wall_ns=int(fields[1]),checksum=int(fields[2]),stderr=result.stderr))
  assert len({r['checksum'] for r in rows if r['fixture']==fixture})==1
 (p/'results/raw.json').write_text(json.dumps(rows,indent=2)+'\n')
 print(fixture,flush=True)
