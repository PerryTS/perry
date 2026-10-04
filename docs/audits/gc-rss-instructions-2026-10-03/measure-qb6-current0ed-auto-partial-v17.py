"""Measure the 16 buildable normal-auto cases; request failure stays excluded.
This partial matrix cannot satisfy the full 17-case acceptance guard.
"""
import json,subprocess
import bench
import importlib.util
B=bench.B; Q=B/'qb6-current0ed-build-v14'; V='gc-auto-current0ed-qb6-v17-partial'; BV='gc-auto-qb6-current0ed-v14r1';rows=[]
arms=['current0ed-base','current0ed-gc']
cases=list(bench.CASES)+[(n,B/'extra-sources'/f'{n}.ts',[]) for n in ['00-noop','15-crc32']]
assert len(cases)==17 and len({c[0] for c in cases})==17
excluded=[c for c in cases if c[0]=='72-request-processing'];cases=[c for c in cases if c[0]!='72-request-processing'];assert len(cases)==16
assert not (B/(V+'-runs.json')).exists()
assert (Q/'qb6-current0ed-auto-builds-r1.exit').read_text().strip()=='1'
completion=json.loads((Q/(BV+'-completion.json')).read_text());assert completion['build_records']==30 and completion['successful']==28
failed=completion['failed_sources'];assert {(r['arm'],__import__('pathlib').Path(r['source']).stem) for r in failed}=={(a,'72-request-processing') for a in arms}
manifest=Q/(BV+'-builds.json');original=json.loads(manifest.read_text());assert len(original)==30
products=json.loads((Q/(BV+'-products.json')).read_text());local_products={str(Q/__import__('pathlib').Path(p).relative_to('/root/rss-gc-build-20261003/export-current0ed-v14')):h for p,h in products.items()};assert all(bench.m.sha(p)==h for p,h in local_products.items());bench.m.save(V+'-products.json',local_products)
proof=json.loads((B/'qb6-current0ed-build-v14-verification.json').read_text());assert all(local_products[str(Q/('current0ed-'+a)/n)]==proof['products'][a+'/'+n] for a in ['base','gc'] for n in ['perry','libperry_runtime.a','libperry_stdlib.a'])
binaries={};builds=[]
for arm in arms:
 for name,source,args in cases:
  hits=[r for r in original if r['arm']==arm and __import__('pathlib').Path(r['source']).stem==source.stem];assert len(hits)==1; r=hits[0]
  assert r['rc']==0 and r['reason'] is None and bench.m.sha(source)==r['source_sha256']
  binary=Q/'bin'/f'{BV}-{arm}-{source.stem}';symbol=__import__('pathlib').Path(str(binary)+'-symbols')
  assert bench.m.sha(binary)==r['binary_sha256'] and bench.m.sha(symbol)==r['symbol_sha256']
  assert bench.m.sha(Q/(binary.name+'.text'))==r['text_sha256']==bench.m.sha(Q/(symbol.name+'.text'))
  if arm=='current0ed-gc' and source.stem in ['tscwork','30-string-build']: assert r.get('pool_release_symbol_witness') and r.get('reuse_window_symbol_witness')
  binaries[arm,name]=binary
  if not any(b['arm']==arm and b['source_sha256']==r['source_sha256'] for b in builds): builds.append(dict(r,reused_from_manifest=str(manifest),reuse_manifest_sha256=bench.m.sha(manifest),build_host='qb6',measurement_host='ideal-mastodon'))
bench.m.save(V+'-builds.json',builds)
previous=json.loads((Q/(BV+'-oracle.json')).read_text());oracles={}
for name,source,args in cases:
 r=bench.run(V+'-node-'+name,['node','--expose-gc',source,*args],timeout=300);assert r['rc']==0 and r['reason'] is None and r['stdout']==previous[name];oracles[name]=r['stdout']
bench.m.save(V+'-oracle.json',oracles)
bench.m.save(V+'-scope.json',dict(complete=False,full_suite_cases=17,measured_cases=16,excluded_cases=[c[0] for c in excluded],failed_builds=failed,criterion='Partial diagnostic matrix only. Pristine baseline and GC candidate both fail LLVM parsing for request-processing; no partial linking and no full-suite acceptance.'))
for mode in ['plain','perf']:
 for repeat in range(3):
  for name,source,args in cases:
   for arm in (arms if repeat%2==0 else arms[::-1]):
    binary=binaries[arm,name];label=f'{V}-{mode}-{arm}-{name}-{repeat}'
    if mode=='plain':cmd=['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary,*args]
    else:cmd=['taskset','-c','2','perf','stat','-x,','-e','instructions,cycles,minor-faults,major-faults',binary,*args]
    r=bench.run(label,cmd,timeout=300)
    r.update(case=name,arm=arm,mode=mode,repeat=repeat,binary_sha256=bench.m.sha(binary),correct=r['rc']==0 and r['reason'] is None and r['stdout']==oracles[name])
    if mode=='perf':
     events={}
     for line in (B/'logs'/(label+'.err')).read_text().splitlines():
      fields=line.split(',')
      if len(fields)>2 and fields[2] in ['instructions','cycles','minor-faults','major-faults']:events[fields[2]]=int(fields[0])
     r['events']=events
    rows.append(r);bench.m.save(V+'-runs.json',rows);assert r['correct'],r
    if mode=='perf':assert len(r['events'])==4,r
    else:assert r['peak_rss_bytes'] is not None,r
    print(mode,arm,name,repeat,r.get('peak_rss_bytes'),r.get('events'),flush=True)
assert len(rows)==192
assert all(bench.m.sha(p)==h for p,h in local_products.items())
spec=importlib.util.spec_from_file_location('comparison',B/'summarize-gc-comparison.py');mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
mod.validate(rows,oracles,arms,tuple(c[0] for c in cases),True);mod.validate_builds(rows,builds)
for (arm,name),binary in binaries.items():assert bench.m.sha(binary)==next(r['binary_sha256'] for r in rows if r['arm']==arm and r['case']==name)
bench.m.save(V+'-summary.json',dict(suite='auto-partial',baseline=arms[0],candidate=arms[1],complete_cells=192,full_acceptance_complete=False,excluded_cases=['72-request-processing'],inputs={n:bench.m.sha(B/(V+'-'+n+'.json')) for n in ['runs','builds','oracle','scope']},cases=mod.summarize(rows,arms,tuple(c[0] for c in cases),True),limitations='Three repetitions, shared host; partial suite, request-processing unmeasured, no automatic acceptance verdict.'))
(B/(V+'.exit')).write_text('0\n')
