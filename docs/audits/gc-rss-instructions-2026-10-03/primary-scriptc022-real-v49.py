"""Latest ScriptC real-library support/RSS attempt, exact Perry TS/Zod sources."""
import pathlib,json,os,time,subprocess
import bench
B=bench.B;V='scriptc022-real-v49';T=B/'comparetools-scriptc022-v38';R=B/(V+'-owned');R.mkdir(exist_ok=False)
compiler=T/'node_modules/.bin/scriptc'
assert subprocess.check_output([compiler,'--version'],text=True).strip()=='0.2.2'
provenance=json.loads((B/'comparators-scriptc022-v38-verification.json').read_text())
assert bench.m.sha(compiler)==provenance['version']['compiler_sha256']
rows=[];oracles={}
for case,args in [('tscwork',['1']),('zodwork',['200'])]:
 src=B/'sources/real'/(case+'.ts')
 oracle=bench.run(V+'-oracle-'+case,['node','--expose-gc',src,*args],timeout=300);assert oracle['rc']==0 and oracle['reason'] is None;oracles[case]=oracle['stdout']
 for variant,flags in [('default',[]),('static-npm',['--npm-static','auto'])]:
  binary=R/(case+'-'+variant)
  start=time.monotonic()
  while int(next(s.split()[1] for s in open('/proc/meminfo') if s.startswith('MemAvailable:')))<24*2**20:
   assert time.monotonic()-start<7200,'compile headroom timeout'
   time.sleep(30)
  label=V+'-build-'+case+'-'+variant
  build=bench.run(label,[compiler,'build',src,*flags,'-o',binary],timeout=1800)
  build.update(case=case,variant=variant,mode='build',source_sha256=bench.m.sha(src),compiler_sha256=bench.m.sha(compiler))
  build['compiled']=build['rc']==0 and build['reason'] is None and binary.exists();rows.append(build);bench.m.save(V+'-runs.json',rows)
  if not build['compiled']:
   print(case,variant,'compile failed',build['rc'],build['reason'],flush=True);continue
  build['binary_sha256']=bench.m.sha(binary);bench.m.save(V+'-runs.json',rows)
  for rep in range(3):
   row=bench.run(f'{V}-run-{case}-{variant}-{rep}',['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary,*args],timeout=300)
   row.update(case=case,variant=variant,mode='plain',repeat=rep,binary_sha256=bench.m.sha(binary),correct=row['rc']==0 and row['reason'] is None and row['stdout']==oracle['stdout']);rows.append(row);bench.m.save(V+'-runs.json',rows)
   print(case,variant,rep,'correct',row['correct'],flush=True)
   if not row['correct']:break
bench.m.save(V+'-oracle.json',oracles)
bench.m.save(V+'-provenance.json',dict(compiler=provenance['version'],source_verification_sha256=bench.m.sha(B/'comparators-scriptc022-v38-verification.json'),dependency_package_lock_sha256=bench.m.sha(B/'sources/real/package-lock.json'),scope='Two unchanged real applications, default and explicit static npm mode. No dynamic engine or source/library stubs. Unsupported builds or incorrect outputs receive no RSS rank. This support probe is separate from GC acceptance.'))
(B/(V+'.exit')).write_text('0\n')
