"""Latest ScriptC release, seven unchanged workload sources; failures stay visible."""
import pathlib,json,subprocess,os
import bench
B=bench.B;V='comparators-scriptc022-v38';T=B/'comparetools-scriptc022-v38'
assert json.loads((T/'node_modules/scriptc/package.json').read_text())['version']=='0.2.2'
compiler=T/'node_modules/.bin/scriptc'
bench.m.ENV['PATH']='/usr/lib/llvm-22/bin:'+bench.m.ENV['PATH']
version=bench.run(V+'-version',[compiler,'--version']);assert version['rc']==0 and version['stdout'].strip()=='0.2.2'
bench.m.save(V+'-version.json',dict(version=version['stdout'].strip(),compiler_sha256=bench.m.sha(compiler.resolve()),lock_sha256=bench.m.sha(T/'package-lock.json')))
cases=['00-noop','15-crc32','23-binary-trees','30-string-build','31-json','51-pipeline','60-ring-churn']
records=[];oracles={};builds={}
assert not (B/(V+'-runs.json')).exists()
for case in cases:
 src=B/('extra-sources' if case in ['00-noop','15-crc32'] else 'sources')/(case+'.ts')
 oracle=bench.run(V+'-oracle-'+case,['node',src]);assert oracle['rc']==0 and oracle['reason'] is None
 oracles[case]=oracle['stdout'];bench.m.save(V+'-oracle.json',oracles)
 binary=B/'bin'/(V+'-'+case);assert not binary.exists()
 row=bench.run(V+'-build-'+case,[compiler,'build',src,'-o',binary],timeout=300)
 row.update(case=case,engine='scriptc',mode='build',source_sha256=bench.m.sha(src),compiler_sha256=bench.m.sha(compiler.resolve()),compiled=row['rc']==0 and row['reason'] is None and binary.exists())
 if row['compiled']:row['binary_sha256']=bench.m.sha(binary)
 builds[case]=row;records.append(row);bench.m.save(V+'-runs.json',records);print('build',case,row['rc'],flush=True)
for rep in range(3):
 for case in cases:
  build=builds[case]
  if not build['compiled']:continue
  binary=pathlib.Path(build['cmd'][-1])
  row=bench.run(V+'-run-'+case+'-'+str(rep),['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary],timeout=180)
  row.update(case=case,engine='scriptc',mode='plain',repeat=rep,binary_sha256=bench.m.sha(binary),correct=row['rc']==0 and row['reason'] is None and row['stdout']==oracles[case]);records.append(row);bench.m.save(V+'-runs.json',records)
  print('run',case,rep,row['correct'],row['peak_rss_bytes'],flush=True)
assert bench.m.sha(compiler.resolve())==json.loads((B/(V+'-version.json')).read_text())['compiler_sha256']
(B/(V+'.exit')).write_text('0\n')
