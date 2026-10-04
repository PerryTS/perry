"""Normal auto-optimized A/B, untouched GC defaults, same 17 application cases.
One symbol build per source/arm; strip a copy and verify identical .text. Build
records allow verified reuse in later measurement passes without recompiling.
"""
import json,os,pathlib,shutil,subprocess,time,concurrent.futures
import bench
B=bench.B; V='gc-auto-mainad0-v79'
R=B/'primary-mainad0-once-v74'
exports={'base':B/'primary-mainad0-base-v77/export','gc':R/'export'}
ARMS={'base':B/'primary-mainad0-base-v77/source','gc':R/'source'}
for a in ARMS:
    assert (exports[a]/'complete.exit').read_text().strip()=='0'
    assert (exports[a]/'independent-build-verification.json').exists()

def build_free_bytes():
    # Root jobs can use reserved blocks; retain the same 12GiB minimum.
    space=os.statvfs(B)
    return (space.f_bfree if os.geteuid()==0 else space.f_bavail)*space.f_frsize

CASES=list(bench.CASES)+[(n,B/'extra-sources'/f'{n}.ts',[]) for n in ['00-noop','15-crc32']]
assert len(CASES)==17
for a in ARMS:
    inputs=json.loads((exports[a]/'source-inputs.json').read_text())
    assert all(bench.m.sha(ARMS[a]/n)==h for n,h in inputs.items())
builds=json.loads((B/(V+'-builds.json')).read_text()) if (B/(V+'-builds.json')).exists() else []
oracles={}; records=[]
assert not (B/(V+'-runs.json')).exists(),'preserve existing run evidence; use an explicit new run version'
artifacts={str(p):bench.m.sha(p) for a in ARMS for p in [exports[a]/'perry',exports[a]/'libperry_runtime.a',exports[a]/'libperry_stdlib.a']}
bench.m.save(V+'-products.json',artifacts)
for name,src,args in CASES:
    r=bench.run(V+'-oracle-'+name,['node','--expose-gc',src,*args],timeout=300)
    assert r['rc']==0,r
    oracles[name]=r['stdout']
bench.m.save(V+'-oracle.json',oracles)
def paths(arm,src):
    plain=B/'bin'/f'{V}-{arm}-{src.stem}'
    return plain,pathlib.Path(str(plain)+'-symbols')
def build_arm(arm,tree):
    builds=[]
    for src in dict.fromkeys(s for _,s,_ in CASES):
        plain,symbols=paths(arm,src)
        key={'arm':arm,'source':str(src),'source_sha256':bench.m.sha(src),'compiler_sha256':artifacts[str(exports[arm]/'perry')]}
        prior=[r for r in builds if all(r.get(k)==v for k,v in key.items()) and r.get('rc')==0]
        if prior:
            r=prior[-1]
            assert bench.m.sha(plain)==r['binary_sha256'] and bench.m.sha(symbols)==r['symbol_sha256']
            if arm=='gc' and src.stem in ['tscwork','30-string-build']:
                assert r.get('pool_release_symbol_witness'), ('reused binary lacks pool witness',arm,src.stem)
                assert r.get('reuse_window_symbol_witness'), ('reused binary lacks window witness',arm,src.stem)
            continue
        assert not plain.exists() and not symbols.exists(),(plain,symbols)
        assert build_free_bytes() >= 12*2**30, 'need12GiB free before another auto runtime build; preserve results and reclaim only our reproducible build caches'
        start=time.monotonic()
        while int(next(s.split()[1] for s in open('/proc/meminfo') if s.startswith('MemAvailable:')))<24*2**20:
            assert time.monotonic()-start<7200,'compile headroom timeout'
            time.sleep(30)
        env={'PERRY_RUNTIME_DIR':str(exports[arm]),'PERRY_WORKSPACE_ROOT':str(tree),
             'PERRY_NO_CACHE':'1','PERRY_KEEP_SYMBOLS':'1','LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22',
             'CARGO_BUILD_JOBS':'4','CARGO_TARGET_DIR':str(B/('primary-mainad0-base-v77' if arm=='base' else 'primary-mainad0-once-v74')/('target-auto-'+arm)),'RAYON_NUM_THREADS':'2','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0'}
        label=f'{V}-build-{arm}-{src.stem}'
        r=bench.run(label,[exports[arm]/'perry','compile',src,'-o',symbols],env,3600)
        r.update(key,case=src.stem);builds.append(r);bench.m.save(V+'-'+arm+'-builds.json',builds)
        if r['rc']!=0 or r['reason'] is not None:
            print('build failed; preserving and continuing',arm,src.stem,flush=True)
            continue
        if arm=='gc' and src.stem in ['tscwork','30-string-build']:
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
        bench.m.save(V+'-'+arm+'-builds.json',builds)
        # Independently verified later from the preserved binaries, so no large temporary dump must remain.
        for file in [symbols,plain]:(B/(file.name+'.text')).unlink()
        print('built normal-auto',arm,src.stem,flush=True)
    return builds
builds=[]
for a,t in ARMS.items():builds.extend(build_arm(a,t))
bench.m.save(V+'-builds.json',builds)
assert len(builds)==30
auto_products={}
for arm,tree in ARMS.items():
    for stamp in (tree/'target').glob('perry-auto-*/.perry-auto-build.stamp'):
        libraries=[stamp.parent/'release'/n for n in ['libperry_runtime.a','libperry_stdlib.a']]
        auto_products[str(stamp)]={'stamp':stamp.read_text(),'archives':{str(p):bench.m.sha(p) for p in libraries if p.exists()}}
assert auto_products,'normal-auto build stamps missing'
bench.m.save(V+'-auto-runtime-products.json',auto_products)
assert {p:bench.m.sha(p) for p in artifacts}==artifacts
failed=[r for r in builds if r['rc']!=0 or r['reason'] is not None]
bench.m.save(V+'-completion.json',dict(build_records=len(builds),successful=len(builds)-len(failed),failed_sources=[dict(arm=r['arm'],source=r['source'],rc=r['rc'],reason=r['reason']) for r in failed],scope='Complete build coverage; failed builds excluded from acceptance.'))
(B/(V+'-builds.exit')).write_text('1\n' if failed else '0\n')
assert not failed, 'Preserved failed build records; no measurement acceptance'
print('All 30 fresh matched mainad0 builds verified; measuring all 17 cases.',flush=True)

rows=[];binaries={}
for arm in ARMS:
    for name,source,args in CASES:
        binary,symbol=paths(arm,source)
        row=next(x for x in builds if x['arm']==arm and x['source']==str(source))
        assert bench.m.sha(binary)==row['binary_sha256'] and bench.m.sha(symbol)==row['symbol_sha256']
        binaries[arm,name]=binary
arms=list(ARMS)
for mode in ['plain','perf']:
    for rep in range(3):
        for name,source,args in CASES:
            for arm in (arms if rep%2==0 else arms[::-1]):
                binary=binaries[arm,name];label=f'{V}-{mode}-{arm}-{name}-{rep}'
                cmd=['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary,*args] if mode=='plain' else ['taskset','-c','2','perf','stat','-x,','-e','instructions,cycles,minor-faults,major-faults',binary,*args]
                r=bench.run(label,cmd,timeout=300)
                r.update(case=name,arm=arm,mode=mode,repeat=rep,binary_sha256=bench.m.sha(binary),correct=r['rc']==0 and r['reason'] is None and r['stdout']==oracles[name])
                if mode=='perf':
                    r['events']={f[2]:int(f[0]) for line in (B/'logs'/(label+'.err')).read_text().splitlines() if len(f:=line.split(','))>2 and f[2] in ['instructions','cycles','minor-faults','major-faults']}
                    assert len(r['events'])==4
                rows.append(r);bench.m.save(V+'-runs.json',rows);assert r['correct'],r
                print(mode,arm,name,rep,flush=True)
assert len(rows)==204 and all(bench.m.sha(p)==h for p,h in artifacts.items())
import importlib.util
spec=importlib.util.spec_from_file_location('summary',B/'summarize-gc-comparison.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
for pair in [['base','gc']]:m.validate([x for x in rows if x['arm'] in pair],oracles,pair,m.AUTO_CASES,True)
m.validate_builds(rows,builds)
bench.m.save(V+'-summary.json',dict(complete_cells=204,comparisons={'gc_vs_base':m.summarize(rows,['base','gc'],m.AUTO_CASES,True)},scope='Pinned mainad0 baseline versus adopted GC and once-advice changes, 17 normal-auto workloads, no exclusions.',inputs={n:bench.m.sha(B/(V+'-'+n+'.json')) for n in ['runs','builds','oracle','products']}))
(B/(V+'.exit')).write_text('0\n')
