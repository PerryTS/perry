from pathlib import Path
import subprocess,json,random,hashlib
p=Path(__file__).resolve().parent;fixtures=Path.home()/'json-deferred-gc-v2-20260907-codex/.work/fixtures'
cases={name:fixtures/(name+'.json') for name in ['escaped_1m','unicode_1m','long_string_1m','records_object_1m','wide_1m','numbers_1m','small_record','tiny_object','object_1k']}
for name,value in [('small-escaped-array',['a\nb','x"y','\\','\ud800']*5000),('sparse-escapes',{'text':'x\n'+'a'*1000000+'\tend'})]:
 path=p/(name+'.json');path.write_text(json.dumps(value,ensure_ascii=True,separators=(',',':')));cases[name]=path
metadata={'worker_sha256':hashlib.sha256((p/'probe').read_bytes()).hexdigest(),'fixtures':{name:hashlib.sha256(path.read_bytes()).hexdigest() for name,path in cases.items()},'iterations':'100 large; 1000000 under 4096 bytes','repeats':7,'seed':14398,'note':'Standalone nesting-depth preflight, no allocation or GC; not full JSON parsing. Same binary contains both arms.'}
(p/'results/metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
rng=random.Random(metadata['seed']);rows=[]
for name,path in cases.items():
 n=1000000 if path.stat().st_size<4096 else 100
 for rep in range(7):
  order=['before','after'];rng.shuffle(order)
  for position,arm in enumerate(order+order[::-1]):
   result=subprocess.run(['/usr/bin/time','-l',str(p/'probe'),str(path),str(n),arm],capture_output=True,text=True,check=True,timeout=60)
   fields=result.stdout.split();rows.append(dict(fixture=name,iterations=n,rep=rep,position=position,arm=arm,wall_ns=int(fields[1]),checksum=int(fields[2]),stderr=result.stderr))
  assert len({r['checksum'] for r in rows if r['fixture']==name})==1
 (p/'results/raw.json').write_text(json.dumps(rows,indent=2)+'\n');print(name,flush=True)
