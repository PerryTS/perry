"""Fresh instruction repeats on exact V52 controls; no rebuild or GC override."""
import json,pathlib,hashlib,re
import bench
B=bench.B;V='gc-lazy-instruction-repeat-main07-v67';P='gc-auto-lazy-rss-main07-v52'
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert (B/(P+'-driver.exit')).read_text().strip()=='0'
assert not (B/(V+'-runs.json')).exists()
prior=json.loads((B/(P+'-runs.json')).read_text())
builds=json.loads((B/(P+'-builds.json')).read_text())
cases=[('73-retain-then-release',B/'sources/73-retain-then-release.ts',[]),('tscwork',B/'sources/real/tscwork.ts',['1'])]
inputs=dict(prior=P,prior_verification_sha256=sha(B/(P+'-verification.json')),prior_runs_sha256=sha(B/(P+'-runs.json')),prior_builds_sha256=sha(B/(P+'-builds.json')),cases={},scope='Nine fresh perf repetitions per case/arm, exact V52 binaries, no rebuild or GC override.')
for case,src,args in cases:
    oracle=bench.run(V+'-oracle-'+case,['node','--expose-gc',src,*args],timeout=300)
    assert oracle['rc']==0 and oracle['reason'] is None
    selected={}
    for arm in ['gc','lazy']:
        row=next(x for x in prior if x['arm']==arm and x['case']==case and x['mode']=='perf')
        binary=pathlib.Path(row['cmd'][8]);assert sha(binary)==row['binary_sha256']
        build=next(x for x in builds if x['arm']==arm and x['source']==str(src))
        assert build['binary_sha256']==sha(binary) and build['source_sha256']==sha(src)
        assert row['stdout']==oracle['stdout']
        selected[arm]=dict(path=str(binary),sha256=sha(binary))
    inputs['cases'][case]=dict(source=str(src),source_sha256=sha(src),args=args,oracle=oracle['stdout'],binaries=selected)
bench.m.save(V+'-inputs.json',inputs)
rows=[]
for rep in range(9):
    for case,_,args in cases:
        for arm in (['gc','lazy'] if rep%2==0 else ['lazy','gc']):
            binary=inputs['cases'][case]['binaries'][arm]
            r=bench.run(f'{V}-{case}-{arm}-{rep}',['taskset','-c','2','perf','stat','-x,','-e','instructions,cycles,minor-faults,major-faults',binary['path'],*args],timeout=300)
            r.update(arm=arm,case=case,repeat=rep,binary_sha256=sha(binary['path']),correct=r['rc']==0 and r['reason'] is None and r['stdout']==inputs['cases'][case]['oracle'])
            err=(B/'logs'/(r['label']+'.err')).read_text()
            r['events']={f[2]:int(f[0]) for line in err.splitlines() if len(f:=line.split(','))>2 and f[2] in ['instructions','cycles','minor-faults','major-faults']}
            rows.append(r);bench.m.save(V+'-runs.json',rows)
            assert r['correct'] and len(r['events'])==4,r
            print(case,arm,rep,r['events']['instructions'],flush=True)
assert len(rows)==36
(B/(V+'.exit')).write_text('0\n')
