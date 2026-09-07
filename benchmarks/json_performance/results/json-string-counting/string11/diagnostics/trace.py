from pathlib import Path
import os,json,subprocess,hashlib
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');out=Path(__file__).resolve().parent
fixture=art/'root-retention7/records_object_20m.json';rows=[]
for arm in ['ship-checkpoint','string10','string11']:
 if not (out/(arm+'.stderr')).exists():
  env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};env['PERRY_GC_TRACE']='1'
  p=subprocess.run([str(art/(arm+'-lifetime-worker')),str(fixture),'discard','24','roundtrip'],capture_output=True,text=True,env=env,timeout=120)
  (out/(arm+'.stdout')).write_text(p.stdout);(out/(arm+'.stderr')).write_text(p.stderr);assert p.returncode==0
 events=[json.loads(l) for l in (out/(arm+'.stderr')).read_text().splitlines() if l.startswith('{')];events=[e for e in events if e.get('event')=='gc_cycle']
 row=dict(arm=arm,cycles=len(events),full=sum(e['collection_kind']=='full' for e in events),pointer_slots=sum(e['layout_scans']['pointer_slots_read'] for e in events),worker_sha256=hashlib.sha256((art/(arm+'-lifetime-worker')).read_bytes()).hexdigest(),fixture_sha256=hashlib.sha256(fixture.read_bytes()).hexdigest());rows.append(row)
 (out/'summary.json').write_text(json.dumps(rows,indent=2)+'\n');print(row,flush=True)
