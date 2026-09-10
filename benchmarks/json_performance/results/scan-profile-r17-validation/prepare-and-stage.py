from pathlib import Path
import hashlib,json,os,shlex,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3];bench=w.parents[1];old=w.parent/'template-capture-r16';sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()=='1a9c0de6cb790d2467b0ca22a660870025179b37'
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
all_cases=json.loads((old/'full-cases.json').read_text())['cases'];lookup={(c['fixture'],c['operation']):c for c in all_cases};cases=[]
for fixture in ['records_array_16k','records_array_1m','records_array_8m']:
 for op in ['parse','sparse','scan']:
  c=lookup[fixture,op];cases.append(c|{'iterations':c['iterations']*2})
c=lookup['records_array_20m','scan'];cases.append(c|{'iterations':c['iterations']*2})
assert len(cases)==10 and all(c['repetitions']==7 for c in cases)
(w/'mode-cases.json').write_text(json.dumps({'cases':cases,'selection':'Predeclared diagnostic: parse/sparse/scan at 16 KiB, 1 MiB and 8 MiB, plus a 20 MiB scan same-route control. Two times original full-matrix work, original warmup, seven repetitions. No runtime candidate or qualification claim.'},indent=2)+'\n')
for p in w.glob('*.py'):compile(p.read_text(),p.name,'exec')
for r in json.loads((w/'main-workers-provenance.json').read_text()):
 for p,digest in r['files'].items():assert sha(root/p)==digest,p
assert (w/'harness/worker.ts').read_bytes()==(old/'harness/worker.ts').read_bytes()
(w/'validation-reference.json').write_text(json.dumps({'source_commit':'1a9c0de6cb790d2467b0ca22a660870025179b37','worker_source_matches_r16_main':True,'cross_path_object_equal':False,'worker_object_sha256':sha(w/'main-worker.o'),'reference':'https://github.com/PerryTS/perry/blob/3378e00c3ef9690c641f2d3dbef0b4f24f17a571/benchmarks/json_performance/results/template-capture-r16-validation/main-fixture-validation.json','note':'Unchanged main runtime and worker source; generated objects embed different source paths, so cross-path object equality is not claimed. Both modes use the same R17 executable. R16 main checks covered auto/tape/direct modes under scheduled moving/protected GC. New diagnostics verify complete outputs for every mode/case.'},indent=2)+'\n')
fixtures=['.work/fixtures/'+name+'.json' for name in ['records_array_16k','records_array_1m','records_array_8m','records_array_20m']]
(w/'fixture-hashes.json').write_text(json.dumps({p:sha(bench/p) for p in fixtures},indent=2)+'\n')
files=['with_lock.py','run_dispatch_focus.py']+fixtures+[str((w/n).relative_to(bench)) for n in ['run-modes.py','mode-cases.json','main-worker','main-workers-provenance.json','main-build-provenance.json','reference-main.json','validation-reference.json','fixture-hashes.json','harness/worker.ts','harness/worker.js']]
expected={p:sha(bench/p) for p in files};host='perry@perry-macos.local';remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';token='json-r17-stage-'+str(os.getpid())
def ssh(code):subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
ssh('from pathlib import Path;p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text('+repr(token)+')')
try:
 subprocess.run(['rsync','-aR']+files+[host+':'+remote+'/'],cwd=bench,check=True)
 ssh('from pathlib import Path;import hashlib;r=Path('+repr(remote)+');e='+repr(expected)+';assert {p:hashlib.sha256((r/p).read_bytes()).hexdigest() for p in e}==e;print("VERIFIED",len(e),"diagnostic input hashes")')
 (w/'remote-stage-hashes.json').write_text(json.dumps(expected,indent=2)+'\n')
finally:ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()=='+repr(token)+';(p/"owner").unlink();p.rmdir()')
