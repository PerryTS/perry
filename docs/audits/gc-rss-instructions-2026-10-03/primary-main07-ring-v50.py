"""Unchanged ring workload: full-runtime default run versus natural GC trace."""
import pathlib,json,subprocess,os,time
import bench
B=bench.B;V='gc-main07-ring-v50';R=B/'primary-main07-v39';candidate=B/'primary-entry-once-main07-v45'
exports={'gc':R/'export/gc','entry':R/'export/entry','once':candidate/'export'}
trees={'gc':R/'source-gc','entry':R/'source-entry','once':candidate/'source'}
assert (candidate/'export/independent-build-verification.json').exists()
source=B/'sources/60-ring-churn.ts';oracle=bench.run(V+'-oracle',['node',source],timeout=300);assert oracle['rc']==0 and oracle['reason'] is None
rows=[];products={str(p):bench.m.sha(p) for e in exports.values() for p in [e/'perry',e/'libperry_runtime.a',e/'libperry_stdlib.a']}
bench.m.save(V+'-products.json',products);bench.m.save(V+'-oracle.json',oracle)
for arm in exports:
 manifest=candidate/'export/source-inputs.json' if arm=='once' else R/'export'/(arm+'-source-inputs.json')
 inputs=json.loads(manifest.read_text());assert len(inputs)==(5011 if arm=='once' else 5010) and all(bench.m.sha(trees[arm]/n)==h for n,h in inputs.items())
 binary=B/'bin'/(V+'-'+arm);assert not binary.exists()
 env={'PERRY_RUNTIME_DIR':str(exports[arm]),'PERRY_WORKSPACE_ROOT':str(trees[arm]),'PERRY_NO_CACHE':'1','PERRY_KEEP_SYMBOLS':'1','RAYON_NUM_THREADS':'2','CARGO_TARGET_DIR':str(B/(V+'-target-'+arm))}
 row=bench.run(V+'-build-'+arm,[exports[arm]/'perry','compile',source,'--no-auto-optimize','-o',binary],env,600)
 row.update(arm=arm,mode='build',source_sha256=bench.m.sha(source),source_manifest_sha256=bench.m.sha(manifest));rows.append(row);bench.m.save(V+'-runs.json',rows)
 assert row['rc']==0 and row['reason'] is None
 row['binary_sha256']=bench.m.sha(binary)
 for mode in ['plain','trace']:
  runtime_env={} if mode=='plain' else {'PERRY_GC_TRACE':'1','PERRY_GC_DIAG':'1'}
  label=V+'-'+mode+'-'+arm
  run=bench.run(label,['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary],runtime_env,300)
  run.update(arm=arm,mode=mode,binary_sha256=bench.m.sha(binary),correct=run['rc']==0 and run['reason'] is None and run['stdout']==oracle['stdout']);rows.append(run);bench.m.save(V+'-runs.json',rows)
  assert run['correct']
  if mode=='trace':
   cycles=[json.loads(line) for line in (B/'logs'/(label+'.err')).read_text().splitlines() if line.startswith('{')]
   assert cycles and all('arena_bytes' in c and 'copying_nursery' in c for c in cycles)
   bench.m.save(V+'-'+arm+'-cycles.json',cycles)
   print(arm,'natural cycles',len(cycles),'copied',sum(c['copying_nursery']['copied_objects'] for c in cycles),flush=True)
 assert all(bench.m.sha(trees[arm]/n)==h for n,h in inputs.items())
assert all(bench.m.sha(p)==h for p,h in products.items())
bench.m.save(V+'-scope.json',dict(base='07e50b14a3d23e8f914193ec4d5a679adbd7c37a',private_once='8e3013733da37ddc66998ee51b9bd1109a9175ec',source_sha256=bench.m.sha(source),scope='Ring attribution diagnostic, three fresh full-runtime builds, same source/output and untouched collection settings. Plain and trace invocations kept separately. Trace logging can perturb residency/pacing; one repetition each is not peak-RSS acceptance or an absolute floor. Auto-optimized comparison remains separate.'))
(B/(V+'.exit')).write_text('0\n')
