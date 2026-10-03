"""Normal auto-optimized A/B, untouched GC defaults, same 17 application cases.
One symbol build per source/arm; strip a copy and verify identical .text. Build
records allow verified reuse in later measurement passes without recompiling.
"""
import json,os,pathlib,shutil,subprocess,time
import bench
B=bench.B; V='gc-auto-currente532-v12'
ARMS={'currente532-base':'perry-currente532-base','currente532-gc':'perry-currente532-gc'}
CASES=list(bench.CASES)+[(n,B/'extra-sources'/f'{n}.ts',[]) for n in ['00-noop','15-crc32']]
assert len(CASES)==17
candidate_manifest=json.loads((B/'provenance-gc-runtime-currente532-v12.json').read_text())
for file,sha in candidate_manifest['files'].items():
    assert bench.m.sha(B/'perry-currente532-gc'/file)==sha,file
builds=json.loads((B/(V+'-builds.json')).read_text()) if (B/(V+'-builds.json')).exists() else []
oracles={}; records=[]
assert not (B/(V+'-runs.json')).exists(),'preserve existing run evidence; use an explicit new run version'
artifacts={str(p):bench.m.sha(p) for a in ARMS for p in [B/a/'perry',B/a/'libperry_runtime.a',B/a/'libperry_stdlib.a']}
bench.m.save(V+'-products.json',artifacts)
for name,src,args in CASES:
    r=bench.run(V+'-oracle-'+name,['node','--expose-gc',src,*args],timeout=300)
    assert r['rc']==0,r
    oracles[name]=r['stdout']
bench.m.save(V+'-oracle.json',oracles)
def paths(arm,src):
    plain=B/'bin'/f'{V}-{arm}-{src.stem}'
    return plain,pathlib.Path(str(plain)+'-symbols')
for arm,tree in ARMS.items():
    for src in dict.fromkeys(s for _,s,_ in CASES):
        plain,symbols=paths(arm,src)
        key={'arm':arm,'source':str(src),'source_sha256':bench.m.sha(src),'compiler_sha256':artifacts[str(B/arm/'perry')]}
        prior=[r for r in builds if all(r.get(k)==v for k,v in key.items()) and r.get('rc')==0]
        if prior:
            r=prior[-1]
            assert bench.m.sha(plain)==r['binary_sha256'] and bench.m.sha(symbols)==r['symbol_sha256']
            if arm not in ['mainrefresh-base','currente532-base'] and src.stem in ['tscwork','30-string-build']:
                assert r.get('pool_release_symbol_witness'), ('reused binary lacks pool witness',arm,src.stem)
                assert r.get('reuse_window_symbol_witness'), ('reused binary lacks window witness',arm,src.stem)
            continue
        assert not plain.exists() and not symbols.exists(),(plain,symbols)
        assert shutil.disk_usage(B).free >= 12*2**30, 'need12GiB free before another auto runtime build; preserve results and reclaim only our reproducible build caches'
        start=time.monotonic()
        while int(next(s.split()[1] for s in open('/proc/meminfo') if s.startswith('MemAvailable:')))<18*2**20:
            assert time.monotonic()-start<7200,'compile headroom timeout'
            time.sleep(30)
        env={'PERRY_RUNTIME_DIR':str(B/arm),'PERRY_WORKSPACE_ROOT':str(B/tree),
             'PERRY_NO_CACHE':'1','PERRY_KEEP_SYMBOLS':'1','LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22',
             'CARGO_BUILD_JOBS':'4','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0'}
        label=f'{V}-build-{arm}-{src.stem}'
        r=bench.run(label,[B/arm/'perry','compile',src,'-o',symbols],env,3600)
        r.update(key);builds.append(r);bench.m.save(V+'-builds.json',builds)
        assert r['rc']==0 and r['reason'] is None,r
        if arm not in ['mainrefresh-base','currente532-base'] and src.stem in ['tscwork','30-string-build']:
            defined=subprocess.check_output(['nm','--defined-only',symbols],stderr=subprocess.DEVNULL)
            assert b'decommit' in defined, ('new pool code missing from linked program',arm,src.stem)
            r['pool_release_symbol_witness']=True
            assert b'advance_block_pool_reuse_window' in defined, ('reuse window absent from linked program',src.stem)
            r['reuse_window_symbol_witness']=True
            assert b'visit_gc_rewrite_slot_descriptors_inline' not in defined, 'rejected visitor helper remains'
        shutil.copy2(symbols,plain)
        subprocess.run(['strip','--strip-all',plain],check=True)
        text_hashes=[]
        for file in [symbols,plain]:
            dumped=B/(file.name+'.text')
            subprocess.run(['objcopy','--dump-section',f'.text={dumped}',file],check=True)
            text_hashes.append(bench.m.sha(dumped))
        assert text_hashes[0]==text_hashes[1]
        r.update(binary_sha256=bench.m.sha(plain),symbol_sha256=bench.m.sha(symbols),text_sha256=text_hashes[0])
        bench.m.save(V+'-builds.json',builds)
        print('built normal-auto',arm,src.stem,flush=True)
for mode in ['plain','perf']:
    for rep in range(3):
        for name,src,args in CASES:
            for arm in (list(ARMS) if rep%2==0 else list(ARMS)[::-1]):
                binary=paths(arm,src)[0];label=f'{V}-{mode}-{arm}-{name}-{rep}'
                if mode=='plain':cmd=['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary,*args]
                else:cmd=['taskset','-c','2','perf','stat','-x,','-e','instructions,cycles,minor-faults,major-faults',binary,*args]
                r=bench.run(label,cmd,timeout=300)
                r.update(case=name,arm=arm,mode=mode,repeat=rep,binary_sha256=bench.m.sha(binary),correct=r['rc']==0 and r['reason'] is None and r['stdout']==oracles[name])
                if mode=='perf':
                    events={}
                    for line in (B/'logs'/(label+'.err')).read_text().splitlines():
                        fields=line.split(',')
                        if len(fields)>2 and fields[2] in ['instructions','cycles','minor-faults','major-faults']: events[fields[2]]=int(fields[0])
                    r['events']=events
                records.append(r);bench.m.save(V+'-runs.json',records)
                assert r['correct'],r
                if mode=='perf':assert len(r['events'])==4,r
                else:assert r['peak_rss_bytes'] is not None,r
                print(mode,arm,name,rep,r.get('peak_rss_bytes'),r.get('events'),flush=True)
auto_products={}
for arm,tree in ARMS.items():
    for stamp in (B/tree/'target').glob('perry-auto-*/.perry-auto-build.stamp'):
        libraries=[stamp.parent/'release'/n for n in ['libperry_runtime.a','libperry_stdlib.a']]
        auto_products[str(stamp)]={'stamp':stamp.read_text(),'archives':{str(p):bench.m.sha(p) for p in libraries if p.exists()}}
assert auto_products,'normal auto runtimes must have build stamps'
bench.m.save(V+'-auto-runtime-products.json',auto_products)
assert len(records)==204
assert {p:bench.m.sha(p) for p in artifacts}==artifacts
