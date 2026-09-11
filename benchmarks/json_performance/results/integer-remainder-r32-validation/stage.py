from pathlib import Path
import hashlib,json,os,shlex,subprocess
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent;bench=w.parents[1];host='perry@perry-macos.local';remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
assert head==json.loads((w/'build-provenance.json').read_text())['source_commit']
units=json.loads((w/'unit-source.json').read_text())
assert units['source_commit']==head and units['exit_code']==0
assert units['command']==['cargo','test','--release','-p','perry-runtime','--lib','json']
assert units['env']=={'RUST_TEST_THREADS':'1'}
arithmetic=json.loads((w/'arithmetic-suite-provenance.json').read_text());assert arithmetic['source_commit']==head and arithmetic['exit_code']==0
for arm in ['main','candidate']:
 fixtures=json.loads((w/(arm+'-fixture-validation.json')).read_text())
 options=json.loads((w/(arm+'-options-validation.json')).read_text())
 assert len(fixtures)==46 and len(options)==14
 assert all(r['matches_node'] and r['exit_code']==0 for r in fixtures)
 assert all(r['protected_retired_sets']>0 and r['moved_objects']>0 for r in fixtures if r['gc']=='scheduled')
 assert all(r['matches_node'] for r in options)
 assert all(r['protected_retired_sets']>0 and r['moved_objects']>0 for r in options if r['mode']=='scheduled')
assert all(r['matches_node'] and r['exit_code']==0 for r in json.loads((w/'candidate-fixture-validation.json').read_text()))
assert all(r['matches_node'] for r in json.loads((w/'candidate-options-validation.json').read_text()))
assert json.loads((w/'root-comparison.json').read_text())['checks_match']
for kind in ['main','candidate']:
 checks=json.loads((w/(kind+'-roots.json')).read_text())['checks']
 assert all(r['exit_code']==0 for r in checks if r['mode']=='shadow' or r['scope'] in ['ordinary-workers','callback'])
for arm in ['main','candidate']:
 for record in json.loads((w/(arm+'-workers-provenance.json')).read_text()):
  for path,digest in record['files'].items():assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
base_rows=json.loads((w/'lazy-main-probes.json').read_text())['rows'];candidate_rows=json.loads((w/'lazy-candidate-probes.json').read_text())['rows'];assert base_rows==candidate_rows and len(base_rows)==24
for row in base_rows:
 stem=str(row['count'])+'-'+row['form']+'-'+row['mode']
 assert (w/('candidate-lazy-'+stem+'.stdout')).read_bytes()==(w/('lazy-'+stem+'.stdout')).read_bytes(),stem
for name in ['worker','access-worker','rotating-worker','options']:
 assert (w/('main-'+name+'.o')).read_bytes()==(w/('candidate-'+name+'.o')).read_bytes(),name
for arm in ['main','candidate']:
 zero=json.loads((w/(arm+'-zero-validation.json')).read_text())['rows']
 assert len(zero)==9 and all(r['exit_code']==0 and r['matches_node'] for r in zero)
 assert all(r['protected_retired_sets']>0 and r['moved_objects']>0 for r in zero if r['mode']=='scheduled')
assert json.loads((w/'zero-root-comparison.json').read_text())['matches']
assert json.loads((w/'additional-root-comparison.json').read_text())['matches']
for arm in ['main','candidate']:
 rows=json.loads((w/(arm+'-changing-options-validation.json')).read_text())
 assert len(rows)==4 and all(r['matches_node'] for r in rows)
 assert all(r['moved_objects']>0 and r['protected_retired_sets']>0 for r in rows if r['mode']=='scheduled')
 for record in json.loads((w/(arm+'-changing-workers-provenance.json')).read_text()):
  for path,digest in record['files'].items():assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
assert (w/'main-changing-options.o').read_bytes()==(w/'candidate-changing-options.o').read_bytes()
for arm in ['main','candidate']:
 rows=json.loads((w/(arm+'-retained-zero-validation.json')).read_text())
 assert len(rows)==8 and all(r['matches_node'] for r in rows)
 assert all(r['moved_objects']>0 and r['protected_retired_sets']>0 for r in rows if r['mode']=='scheduled')
 for record in json.loads((w/(arm+'-retained-zero-workers-provenance.json')).read_text()):
  for path,digest in record['files'].items():assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
assert (w/'main-retained-zero.o').read_bytes()==(w/'candidate-retained-zero.o').read_bytes()
subprocess.run(['python3',str(w/'compare-remainder.py')],cwd=root,check=True)
base=json.loads((w/'base.json').read_text())['base_commit'];(w/'source.patch').write_bytes(subprocess.check_output(['git','diff',base,head],cwd=root))
meta={'source_commit':head,'base_commit':base,'main_build':json.loads((w/'main-build-provenance.json').read_text()),'candidate_build':json.loads((w/'build-provenance.json').read_text())}
(w/'provenance.json').write_text(json.dumps(meta,indent=2)+'\n')
fixture_files=['.work/fixtures/'+r['name']+'.json' for r in json.loads((bench/'results/fixtures.json').read_text())]
fixture_hashes={p:hashlib.sha256((bench/p).read_bytes()).hexdigest() for p in fixture_files}
(w/'fixture-hashes.json').write_text(json.dumps(fixture_hashes,indent=2)+'\n')
files=['run_dispatch_focus.py','with_lock.py','results/fixtures.json',str((w/'fixture-hashes.json').relative_to(bench))]+fixture_files
files += [str(p.relative_to(bench)) for p in w.rglob('*') if p.is_file() and ('harness' in p.parts or p.name in ['main-worker','main-access-worker','main-rotating-worker','main-options','candidate-worker','candidate-access-worker','candidate-rotating-worker','candidate-options','run-access.py','run-focus.py','run-options.py','run-retained.py','options-worker.ts','options-worker.js','provenance.json','source.patch','main-build-provenance.json','main-reuse-provenance.json','main-fixture-validation.json','build-provenance.json','main-workers-provenance.json','candidate-workers-provenance.json','candidate-fixture-validation.json','candidate-options-validation.json','main-options-validation.json','root-comparison.json','main-roots.json','candidate-roots.json','lazy-main-probes.json','lazy-candidate-probes.json','main-zero-validation.json','candidate-zero-validation.json','main-zero-roots.json','candidate-zero-roots.json','zero-root-comparison.json'])]
rotating=json.loads((bench/'.work/rotating/manifest.json').read_text())
wanted={'unicode_1m','long_string_1m','escaped_1m','small_record','records_array_1m'}
files += ['.work/rotating/manifest.json',str((w/'run-rotating.py').relative_to(bench))]
for row in rotating:
 if row['fixture'] in wanted:
  for i,digest in enumerate(row['sha256']):
   p='.work/rotating/'+row['fixture']+'.'+str(i)+'.json'
   assert hashlib.sha256((bench/p).read_bytes()).hexdigest()==digest
   files.append(p)
files += [str((w/name).relative_to(bench)) for name in ['run-changing-options.py','changing-options-worker.ts','changing-options-worker.js','main-changing-options','candidate-changing-options','main-changing-workers-provenance.json','candidate-changing-workers-provenance.json','main-changing-options-validation.json','candidate-changing-options-validation.json']]
files += [str((w/name).relative_to(bench)) for name in ['run-zero-retained.py','retained-zero-worker.ts','retained-zero-worker.js','main-retained-zero','candidate-retained-zero','main-retained-zero-workers-provenance.json','candidate-retained-zero-workers-provenance.json','main-retained-zero-validation.json','candidate-retained-zero-validation.json']]
files += [str((w/name).relative_to(bench)) for name in ['main-changing-roots.json','candidate-changing-roots.json','main-retained-roots.json','candidate-retained-roots.json','additional-root-comparison.json']]
files += [str((w/name).relative_to(bench)) for name in ['main-remainder-validation.json','candidate-remainder-validation.json','main-remainder-roots.json','candidate-remainder-roots.json','remainder-comparison.json']]
files += [str((w/name).relative_to(bench)) for name in ['build-copy-provenance.json','arithmetic-suite-provenance.json']]
expected={p:hashlib.sha256((bench/p).read_bytes()).hexdigest() for p in files};token='json-r32-stage-'+str(os.getpid())
def ssh(code):return subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
ssh('from pathlib import Path; p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text('+repr(token)+')')
try:
 subprocess.run(['rsync','-aR']+files+[host+':'+remote+'/'],cwd=bench,check=True)
 ssh('from pathlib import Path;import hashlib;root=Path('+repr(remote)+');expected='+repr(expected)+';actual={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in expected};assert actual==expected;print("VERIFIED",len(expected),"remote hashes")')
 (w/'remote-stage-hashes.json').write_text(json.dumps(expected,indent=2)+'\n')
finally:ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()=='+repr(token)+';(p/"owner").unlink();p.rmdir()')
