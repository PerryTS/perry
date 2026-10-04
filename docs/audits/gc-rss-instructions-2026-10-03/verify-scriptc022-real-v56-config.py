"""Independent support/correctness/RSS verification: no rank for failures."""
import pathlib,json,hashlib,re,statistics,tarfile
B=pathlib.Path('/root/rss-header-20261002');V='scriptc022-real-v56-config'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def read(n):return json.loads((B/n).read_text())
assert (B/(V+'.exit')).read_text().strip()=='0' and (B/(V+'-driver.exit')).read_text().strip()=='0'
configuration=read(V+'-configuration.json');owned=B/(V+'-owned');assert configuration['config']=={'compilerOptions':{'strict':True,'noImplicitAny':False}};assert configuration['config']==json.loads((owned/'tsconfig.json').read_text());assert configuration['config_sha256']==sha(owned/'tsconfig.json');assert (owned/'node_modules').resolve()==(B/'sources/real/node_modules').resolve();
rows=read(V+'-runs.json');oracle=read(V+'-oracle.json');provenance=read(V+'-provenance.json');prior=read('comparators-scriptc022-v38-verification.json')
assert provenance['compiler']==prior['version'] and provenance['compiler']['version']=='0.2.2'
assert provenance['source_verification_sha256']==sha(B/'comparators-scriptc022-v38-verification.json')
assert provenance['dependency_package_lock_sha256']==sha(B/'sources/real/package-lock.json')
builds=[x for x in rows if x['mode']=='build'];assert len(builds)==4 and {(x['case'],x['variant']) for x in builds}=={(c,v) for c in ['tscwork','zodwork'] for v in ['default','static-npm']}
summary=[];files=set()
for build in builds:
 case=build['case'];variant=build['variant'];src=B/(V+'-owned')/(case+'.ts');assert sha(src)==sha(B/'sources/real'/(case+'.ts'));binary=B/(V+'-owned')/(case+'-'+variant);args=['1'] if case=='tscwork' else ['200']
 assert build['source_sha256']==sha(src) and build['compiler_sha256']==sha(build['cmd'][0])==prior['version']['compiler_sha256']
 flags=[] if variant=='default' else ['--npm-static','auto']
 assert build['cmd'][1:]==['build',str(src),*flags,'-o',str(binary)]
 selected=[r for r in rows if r['mode']=='plain' and r['case']==case and r['variant']==variant]
 if not build['compiled']:
  assert (build['rc']!=0 or build['reason'] is not None) and not selected
  summary.append(dict(case=case,variant=variant,status='compile_failed',rc=build['rc'],reason=build['reason'],rss_mib=None));continue
 assert build['rc']==0 and build['reason'] is None and sha(binary)==build['binary_sha256']
 for r in selected:
  assert r['cmd']==['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',str(binary),*args]
  assert r['binary_sha256']==sha(binary)
  assert r['correct']==(r['rc']==0 and r['reason'] is None and r['stdout']==oracle[case])
  assert int(re.search(r'RSS_KIB=(\d+)',(B/'logs'/(r['label']+'.err')).read_text())[1])*1024==r['peak_rss_bytes']
 correct=len(selected)==3 and all(r['correct'] for r in selected)
 if correct:assert {r['repeat'] for r in selected}=={0,1,2}
 else:assert selected and not selected[-1]['correct']
 summary.append(dict(case=case,variant=variant,status='correct' if correct else 'output_or_execution_failed',runs=len(selected),rss_mib=statistics.median(r['peak_rss_bytes']/2**20 for r in selected) if correct else None))
for r in rows:
 for ext in ['out','err']:files.add(B/'logs'/(r['label']+'.'+ext))
 assert (B/'logs'/(r['label']+'.out')).read_text()==r['stdout']
proof=dict(version='0.2.2',summary=summary,provenance=provenance,configuration=configuration,scope='Unchanged real TS/Zod programs, default and native static-npm modes. Native support/correctness probe; no unsupported/incorrect run has a winning RSS. Passing support would not replace the pending GC full17 comparison.',inputs={n:sha(B/(V+'-'+n+'.json')) for n in ['runs','oracle','provenance','configuration']})
(B/(V+'-verification.json')).write_text(json.dumps(proof,indent=2)+'\n')
files.update([B/(V+'-'+n+'.json') for n in ['runs','oracle','provenance','configuration','verification']]);files.update([B/(V+'.exit'),B/(V+'-driver.exit'),B/(V+'-driver.log'),pathlib.Path(__file__),B/'primary-scriptc022-real-v56-config.py'])
for case in oracle:
 for ext in ['out','err']:files.add(B/'logs'/(V+'-oracle-'+case+'.'+ext))
m=B/(V+'-evidence-files.json');m.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
 for p in sorted(files|{m}):t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(proof))
