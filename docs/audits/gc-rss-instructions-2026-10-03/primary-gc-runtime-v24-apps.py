"""Initial four real-workload comparison of private GC helper probes.
Noauto links each freshly built archive; private tests must finish first.
A passing subset does not establish 17-case production acceptance.
"""
import pathlib,json,subprocess,importlib.util
import bench
B=bench.B;R=B/'primary-gc-runtime-v24';E=R/'export';V='gc-private-currentc170-v24-noauto';arms=['control','weak','elf'];cases=[('tscwork',B/'sources/real/tscwork.ts',['1']),('zodwork',B/'sources/real/zodwork.ts',['200']),('72-request-processing',B/'sources/72-request-processing.ts',[]),('30-string-build',B/'sources/30-string-build.ts',[])]
assert not (B/(V+'-runs.json')).exists();assert (E/'build-driver.exit').read_text().strip()=='0'
for a in arms:assert (E/(a+'-complete.exit')).read_text().strip()=='0'
products={str(E/a/n):h for a in arms for n,h in json.loads((E/(a+'-products.json')).read_text()).items()};assert all(bench.m.sha(p)==h for p,h in products.items());bench.m.save(V+'-products.json',products)
oracles={};builds=[];binaries={}
for name,source,args in cases:
 oracle=bench.run(V+'-node-'+name,['node','--expose-gc',source,*args],timeout=300);assert oracle['rc']==0 and oracle['reason'] is None;oracles[name]=oracle['stdout'];bench.m.save(V+'-oracle.json',oracles)
 for arm in arms:
  binary=B/'bin'/f'{V}-{arm}-{name}';env={'PERRY_RUNTIME_DIR':str(E/arm),'PERRY_WORKSPACE_ROOT':str(R/('source-'+arm)),'CARGO_TARGET_DIR':str(R/('target-app-'+arm)),'PERRY_NO_CACHE':'1','PERRY_KEEP_SYMBOLS':'1','RAYON_NUM_THREADS':'8','LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'8','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0'}
  r=bench.run(V+'-build-'+arm+'-'+name,[E/arm/'perry','compile',source,'--no-auto-optimize','-o',binary],env,1800);r.update(case=name,arm=arm,source_sha256=bench.m.sha(source),compiler_sha256=bench.m.sha(E/arm/'perry'),runtime_sha256=bench.m.sha(E/arm/'libperry_runtime.a'));builds.append(r);bench.m.save(V+'-builds.json',builds);assert r['rc']==0 and r['reason'] is None;r.update(binary_sha256=bench.m.sha(binary));bench.m.save(V+'-builds.json',builds);binaries[arm,name]=binary;print('build',arm,name,'ok',flush=True)
rows=[]
for mode in ['plain','perf']:
 for rep in range(3):
  for name,source,args in cases:
   for arm in (arms[rep:]+arms[:rep]):
    binary=binaries[arm,name];label=f'{V}-{mode}-{arm}-{name}-{rep}'
    cmd=['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary,*args] if mode=='plain' else ['taskset','-c','2','perf','stat','-x,','-e','instructions,cycles,minor-faults,major-faults',binary,*args]
    r=bench.run(label,cmd,timeout=300);r.update(case=name,arm=arm,mode=mode,repeat=rep,binary_sha256=bench.m.sha(binary),correct=r['rc']==0 and r['reason'] is None and r['stdout']==oracles[name])
    if mode=='perf':
     events={}
     for line in (B/'logs'/(label+'.err')).read_text().splitlines():
      f=line.split(',')
      if len(f)>2 and f[2] in ['instructions','cycles','minor-faults','major-faults']:events[f[2]]=int(f[0])
     r['events']=events
    rows.append(r);bench.m.save(V+'-runs.json',rows);assert r['correct'];assert len(r['events'])==4 if mode=='perf' else r['peak_rss_bytes'] is not None;print(mode,arm,name,rep,r.get('events',{}).get('instructions'),flush=True)
assert len(rows)==72;assert all(bench.m.sha(p)==h for p,h in products.items())
spec=importlib.util.spec_from_file_location('s',B/'summarize-gc-comparison.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);
for candidate in ['weak','elf']:
 pair=[r for r in rows if r['arm'] in ['control',candidate]]
 m.validate(pair,oracles,['control',candidate],tuple(c[0] for c in cases),True)
m.validate_builds(rows,builds)
bench.m.save(V+'-summary.json',dict(scope='Private four-workload full-runtime comparison; not full17-case normal-auto acceptance.',complete_cells=72,comparisons={a:m.summarize(rows,['control',a],tuple(c[0] for c in cases),True) for a in ['weak','elf']},inputs={n:bench.m.sha(B/(V+'-'+n+'.json')) for n in ['runs','builds','oracle','products']}));(B/(V+'.exit')).write_text('0\n')
