"""Isolate the consolidated GC patch against freshly fetched main69 on full-runtime TS/Zod.
Normal-auto acceptance is a separate matrix. No collection knobs are changed.
"""
import json, subprocess, pathlib
import bench
B=bench.B; V='gc-shared-v7-noauto'; rows=[]
arms=['main69-base','main69-shared']
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
        if arm=='main69-base':
            prior_rows=json.loads((B/'gc-main69-noauto-builds.json').read_text())
            prior=[r for r in prior_rows if r['arm']==arm and r['case']==case]
            assert len(prior)==1
            r=prior[0]
            binary=pathlib.Path(r['cmd'][-1])
            assert r['rc']==0 and r['reason'] is None
            assert r['source_sha256']==bench.m.sha(src)
            assert r['binary_sha256']==bench.m.sha(binary)
            prior_products=json.loads((B/'gc-main69-noauto-products.json').read_text())
            assert all(prior_products[str(B/arm/n)]==products[str(B/arm/n)] for n in ['perry','libperry_runtime.a','libperry_stdlib.a'])
            builds.append(r.copy());bench.m.save(V+'-builds.json',builds)
            binaries[arm,case]=binary
            continue
        binary=B/'bin'/f'{V}-{arm}-{case}'
        assert not binary.exists()
        env={'PERRY_RUNTIME_DIR':str(B/arm),'PERRY_WORKSPACE_ROOT':str(B/('perry-main69-base' if arm=='main69-base' else 'perry-main69-shared')),'PERRY_NO_CACHE':'1','PERRY_KEEP_SYMBOLS':'1'}
        r=bench.run(V+'-build-'+arm+'-'+case,[B/arm/'perry','compile',src,'--no-auto-optimize','-o',binary],env,600)
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
