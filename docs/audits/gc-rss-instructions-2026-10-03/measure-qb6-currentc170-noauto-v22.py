import json,subprocess
import bench
B=bench.B; Q=B/'qb6-currentc170-build-v22'; V='gc-currentc170-qb6-v22-noauto'; rows=[]
arms=['currentc170-base','currentc170-gc'];cases=[('tscwork',['1']),('zodwork',['200'])]
assert not (B/(V+'-runs.json')).exists()
assert (Q/'qb6-currentc170-real-builds.exit').read_text().strip()=='0'
proof=json.loads((B/'qb6-currentc170-build-v22-verification.json').read_text())
products={str(Q/('currentc170-'+arm)/name):proof['products'][arm+'/'+name] for arm in ['base','gc'] for name in ['perry','libperry_runtime.a','libperry_stdlib.a']};assert all(bench.m.sha(p)==h for p,h in products.items())
bench.m.save(V+'-products.json',products)
manifest=Q/'gc-qb6-currentc170-v22-noauto-builds.json';original=json.loads(manifest.read_text())
assert len(original)==4 and len({(r['case'],r['arm']) for r in original})==4
builds=[];binaries={};oracles={}
for r in original:
    assert r['rc']==0 and r['reason'] is None
    source=B/'sources/real'/(r['case']+'.ts');assert bench.m.sha(source)==r['source_sha256']
    binary=Q/'bin'/('gc-qb6-currentc170-v22-noauto-'+r['arm']+'-'+r['case'])
    assert bench.m.sha(binary)==r['binary_sha256']
    binaries[r['arm'],r['case']]=binary
    builds.append(dict(r,reused_from_manifest=str(manifest),reuse_manifest_sha256=bench.m.sha(manifest),build_host='qb6',measurement_host='ideal-mastodon'))
bench.m.save(V+'-builds.json',builds)
previous_oracles=json.loads((Q/'gc-qb6-currentc170-v22-noauto-oracle.json').read_text())
for case,args in cases:
    r=bench.run(V+'-node-'+case,['node','--expose-gc',B/'sources/real'/(case+'.ts'),*args],timeout=300)
    assert r['rc']==0 and r['reason'] is None and r['stdout']==previous_oracles[case]
    oracles[case]=r['stdout']
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
