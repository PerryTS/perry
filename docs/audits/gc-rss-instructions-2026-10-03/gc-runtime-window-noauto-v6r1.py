"""Isolate the consolidated GC patch against freshly fetched main69 on full-runtime TS/Zod.
Normal-auto acceptance is a separate matrix. No collection knobs are changed.
"""
import json, subprocess
import bench
B=bench.B; V='gc-window-v6r1-noauto'; rows=[]
arms=['main69-gc','main69-window']
cases=[('tscwork',['1']),('zodwork',['200'])]
assert not (B/(V+'-runs.json')).exists()
products={str(p):bench.m.sha(p) for arm in arms for p in [B/arm/n for n in ['perry','libperry_runtime.a','libperry_stdlib.a']]}
bench.m.save(V+'-products.json',products)
binaries={}; oracles={}; builds=[]
for case,args in cases:
    src=B/'sources/real'/(case+'.ts')
    r=bench.run(V+'-oracle-'+case,['node','--expose-gc',src,*args],timeout=300)
    assert r['rc']==0 and r['reason'] is None,r
    oracles[case]=r['stdout']
    for arm in arms:
        binary=B/'bin'/f'{V}-{arm}-{case}'
        if arm=='main69-gc' and case=='tscwork':
            previous_path=B/'gc-window-v6-noauto-builds.json'
            matches=[r for r in json.loads(previous_path.read_text()) if r['arm']==arm and r['case']==case and r['rc']==0 and r['reason'] is None]
            assert len(matches)==1
            prior=matches[0]
            binary=B/'bin'/f'gc-window-v6-noauto-{arm}-{case}'
            assert bench.m.sha(binary)==prior['binary_sha256']
            assert bench.m.sha(src)==prior['source_sha256']
            reused=dict(prior,reused_from_manifest=str(previous_path),reuse_manifest_sha256=bench.m.sha(previous_path))
            builds.append(reused);bench.m.save(V+'-builds.json',builds)
            binaries[arm,case]=binary
            continue
        assert not binary.exists()
        env={'PERRY_RUNTIME_DIR':str(B/arm),'PERRY_WORKSPACE_ROOT':str(B/('perry-main69-gc' if arm=='main69-gc' else 'perry-main69-window')),'PERRY_NO_CACHE':'1','PERRY_KEEP_SYMBOLS':'1'}
        r=bench.run(V+'-build-'+arm+'-'+case,[B/arm/'perry','compile',src,'--no-auto-optimize','-o',binary],env,1800)
        r.update(case=case,arm=arm,source_sha256=bench.m.sha(src));builds.append(r)
        bench.m.save(V+'-builds.json',builds)
        assert r['rc']==0 and r['reason'] is None,r
        r['binary_sha256']=bench.m.sha(binary)
        r['binary_bytes']=binary.stat().st_size
        r['section_sizes']=subprocess.check_output(['size','-A',binary],text=True)
        bench.m.save(V+'-builds.json',builds)
        binaries[arm,case]=binary
bench.m.save(V+'-oracle.json',oracles)
for mode in ['plain','perf']:
    for repeat in range(3):
        for case,args in cases:
            for arm in (arms if repeat%2==0 else arms[::-1]):
                binary=binaries[arm,case];label=f'{V}-{mode}-{arm}-{case}-{repeat}'
                if mode=='plain':cmd=['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary,*args]
                else:cmd=['taskset','-c','2','perf','stat','-x,','-e','instructions,cycles,minor-faults,major-faults',binary,*args]
                r=bench.run(label,cmd,timeout=300)
                r.update(case=case,arm=arm,mode=mode,repeat=repeat,binary_sha256=bench.m.sha(binary),correct=r['rc']==0 and r['reason'] is None and r['stdout']==oracles[case])
                if mode=='perf':
                    events={}
                    for line in (B/'logs'/(label+'.err')).read_text().splitlines():
                        fields=line.split(',')
                        if len(fields)>2 and fields[2] in ['instructions','cycles','minor-faults','major-faults']:events[fields[2]]=int(fields[0])
                    r['events']=events
                rows.append(r);bench.m.save(V+'-runs.json',rows)
                assert r['correct'],r
                if mode=='perf':assert len(r['events'])==4,r
                else:assert r['peak_rss_bytes'] is not None,r
                print(mode,arm,case,repeat,r.get('peak_rss_bytes'),r.get('events'),flush=True)
assert len(rows)==24
assert {p:bench.m.sha(p) for p in products}==products
