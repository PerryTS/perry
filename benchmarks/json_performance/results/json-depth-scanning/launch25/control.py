from pathlib import Path
import hashlib,json,os,random,re,subprocess

root=Path(__file__).resolve().parent
source=Path.home()/'json-depth-runtime-v23-20260907-codex/.work/worker-lifetime-candidate'
slot=Path.home()/'json-escape-runtime-v18-20260907-codex/active'
pending=slot.parent/'next-active'
fixtures=Path.home()/'json-deferred-gc-v2-20260907-codex/.work/fixtures'
assert not pending.exists()
os.link(source,pending)
if slot.exists() and os.path.samefile(slot,pending):pending.unlink()
else:pending.replace(slot)
assert os.path.samefile(source,slot)
digest=hashlib.sha256(source.read_bytes()).hexdigest()
cases=[('small_record','retain',100000,'stringify'),('escaped_1m','latest',100,'parse'),('records_object_20m','discard',24,'roundtrip')]
(root/'results/metadata.json').write_text(json.dumps(dict(worker_sha256=digest,launcher_sha256=hashlib.sha256((root/'launch').read_bytes()).hexdigest(),cases=cases,repeats=7,seed=924712,note='Same runtime binary and exact argv in both arms; hardlink installed once, never changed during trials; immutable execv launcher in second arm'),indent=2)+'\n')
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}

def command(method,fixture,mode,count,op):
    argv=[str(slot),str(fixtures/(fixture+'.json')),mode,str(count),op]
    return argv if method=='direct' else [str(root/'launch'),str(source)]+argv

argv0=str(slot)
probe=subprocess.run([str(root/'launch'),str(root/'argv'),argv0,'payload'],env=env,text=True,capture_output=True,check=True)
assert probe.stdout.splitlines()==[argv0,argv0,'payload'],probe.stdout
(root/'results/argv-check.json').write_text(json.dumps(dict(stdout=probe.stdout,stderr=probe.stderr,argv0=argv0,probe_sha256=hashlib.sha256((root/'argv').read_bytes()).hexdigest()),indent=2)+'\n')

rows=[];rng=random.Random(924712)
for fixture,mode,count,op in cases:
    for rep in range(7):
        order=['direct','launcher'];rng.shuffle(order)
        for position,method in enumerate(order+order[::-1]):
            p=subprocess.run(['/usr/bin/time','-l']+command(method,fixture,mode,count,op),env=env,text=True,capture_output=True,check=True,timeout=180)
            r=json.loads(next(line[9:] for line in p.stdout.splitlines() if line.startswith('LIFETIME ')))
            if mode in ['latest','retain']:
                actual=json.loads(next(line[7:] for line in p.stdout.splitlines() if line.startswith('VERIFY ')))
                if op=='stringify':actual=json.loads(actual)
                assert actual==json.loads((fixtures/(fixture+'.json')).read_text())
                r['verified']=True
            r.update(method=method,fixture=fixture,rep=rep,position=position,peak_rss=int(re.search(r'(\d+)\s+maximum resident set size',p.stderr).group(1)))
            rows.append(r)
        (root/'results/raw.json').write_text(json.dumps(rows,indent=2)+'\n')
    print('TIMING',fixture,op,flush=True)

# Separate traced processes diagnose GC work, never contribute timing data.
traces=[]
for rep in range(2):
    for method in ['direct','launcher']:
        p=subprocess.run(command(method,'records_object_20m','discard',24,'roundtrip'),env=dict(env,PERRY_GC_TRACE='1'),capture_output=True,text=True,check=True,timeout=180)
        events=[json.loads(line) for line in p.stderr.splitlines() if line.startswith('{')]
        events=[e for e in events if e.get('event')=='gc_cycle']
        (root/f'results/{method}-{rep}.trace.jsonl').write_text('\n'.join(json.dumps(e) for e in events)+'\n')
        traces.append(dict(method=method,rep=rep,cycles=len(events),full=sum(e['collection_kind']=='full' for e in events),pointer_slots=sum(e['layout_scans']['pointer_slots_read'] for e in events)))
(root/'results/traces.json').write_text(json.dumps(traces,indent=2)+'\n')
assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
print('COMPLETE',len(rows),'timings',len(traces),'separate traces',flush=True)
