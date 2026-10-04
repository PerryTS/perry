"""Independent diagnostic event/raw-binary/oracle binding; no causal claim."""
import pathlib,json,hashlib,statistics,tarfile
B=pathlib.Path('/root/rss-header-20261002');V='gc-entry-v44-instruction-split';P='gc-auto-latest2026-v33r1'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def read(n):return json.loads((B/n).read_text())
assert (B/(V+'.exit')).read_text().strip()=='0' and (B/(V+'-driver.exit')).read_text().strip()=='0'
rows=read(V+'-runs.json');scope=read(V+'-scope.json');builds=read(P+'-builds.json');oracles=read(P+'-oracle.json')
assert len(rows)==18 and scope['source_verification_sha256']==sha(B/(P+'-verification.json'))
keys={(r['case'],r['arm'],r['repeat']) for r in rows}
assert keys=={(c,a,i) for c in ['tscwork','23-binary-trees','72-request-processing'] for a in ['gc','entry'] for i in range(3)}
events=['instructions:u','instructions:k','cycles:u','cycles:k','minor-faults','major-faults'];running=[];files=set()
for row in rows:
 assert row['rc']==0 and row['reason'] is None and row['correct'] and row['stdout']==oracles[row['case']]
 build=next(x for x in builds if x['arm']==row['arm'] and pathlib.Path(x['source']).stem==row['case'])
 binary=pathlib.Path(row['cmd'][8]);assert sha(binary)==row['binary_sha256']==build['binary_sha256']
 assert row['cmd'][:8]==['taskset','-c','2','perf','stat','-x,','-e',','.join(events)]
 assert (B/'logs'/(row['label']+'.out')).read_text()==row['stdout']
 err=B/'logs'/(row['label']+'.err');parsed={};pct={}
 for line in err.read_text().splitlines():
  f=line.split(',')
  if len(f)>4 and f[2] in events:
   parsed[f[2]]=int(f[0]);pct[f[2]]=float(f[4]);assert 0<pct[f[2]]<=100
 assert parsed==row['events'] and set(parsed)==set(events)
 running.append(dict(case=row['case'],arm=row['arm'],repeat=row['repeat'],running_percent=pct))
 files.update([err,B/'logs'/(row['label']+'.out')])
summary={}
for case in ['tscwork','23-binary-trees','72-request-processing']:
 med={a:{e:statistics.median(r['events'][e] for r in rows if r['case']==case and r['arm']==a) for e in events} for a in ['gc','entry']}
 summary[case]=dict(medians=med,entry_vs_gc_percent={e:100*(med['entry'][e]/med['gc'][e]-1) if med['gc'][e] else None for e in events})
p=dict(base='2026ecfe6dd9df1a0a8616e3bc5c5561e8e0cf63',cells=18,summary=summary,event_running_percent=running,scope='Diagnostic user/kernel split on exact previously accepted binaries, untouched defaults. Multiplexing percentages disclosed. Neither a full-matrix acceptance nor proof that repeated advice causes the cost.',inputs={n:sha(B/n) for n in [V+'-runs.json',V+'-scope.json',P+'-verification.json']})
(B/(V+'-verification.json')).write_text(json.dumps(p,indent=2)+'\n')
files.update([B/(V+'-'+n+'.json') for n in ['runs','scope','verification']]);files.update([B/(V+'.exit'),B/(V+'-driver.exit'),B/(V+'-driver.log'),pathlib.Path(__file__),B/'primary-entry-v44-instruction-split.py'])
m=B/(V+'-evidence-files.json');m.write_text(json.dumps({str(x.relative_to(B)):sha(x) for x in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
 for x in sorted(files|{m}):t.add(x,arcname=str(x.relative_to(B)))
print(json.dumps(p))
