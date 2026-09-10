from pathlib import Path
import gzip,hashlib,json,shlex,subprocess,sys
w=Path(__file__).resolve().parent;bench=w.parents[1];slug=sys.argv[1];kind=sys.argv[2];remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';host='perry@perry-macos.local'
assert kind in ['access','focus','options'] and slug in ['quiet-'+w.name+'-'+kind, 'quiet-'+w.name+'-recheck-'+kind]
code="""from pathlib import Path
import json,shutil
root=Path(REMOTE);w=root/'.work'/'inert-spacer-r8';d=root/'results'/SLUG
window=json.loads((root/'results/window-custom.json').read_text());assert window['quiet_gate_passed'] and window.get('finished_utc') and 'results/'+SLUG in window['command']
for source,name in [(root/'results/window-custom.json','window.json'),(root/'custom.log','controller.log')]:shutil.copy2(source,d/name)
for name in ['processes-before.txt','processes-after.txt']:shutil.copy2(root/'results'/name,d/name)
for name in ['run_dispatch_focus.py','with_lock.py']:shutil.copy2(root/name,d/name)
for name in ['provenance.json','source.patch','main-build-provenance.json','build-provenance.json','main-workers-provenance.json','candidate-workers-provenance.json','candidate-fixture-validation.json','candidate-options-validation.json','main-options-validation.json','root-comparison.json','main-roots.json','candidate-roots.json','lazy-main-probes.json','lazy-candidate-probes.json','run-access.py','run-focus.py','run-options.py','options-worker.ts','options-worker.js']:shutil.copy2(w/name,d/name)
shutil.copytree(w/'harness',d/'harness',dirs_exist_ok=True)
print(window['started_utc'],window['finished_utc'])
""".replace('REMOTE',repr(remote)).replace('SLUG',repr(slug))
subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
subprocess.run(['rsync','-a',host+':'+remote+'/results/'+slug+'/',str(bench/'results'/slug)+'/'],check=True)
d=bench/'results'/slug;p=d/'source.patch';raw=p.read_bytes();data=gzip.compress(raw,compresslevel=9,mtime=0);assert gzip.decompress(data)==raw
p.with_suffix('.patch.gz').write_bytes(data);p.with_suffix('.patch.json').write_text(json.dumps({'original_bytes':len(raw),'original_sha256':hashlib.sha256(raw).hexdigest(),'gzip_sha256':hashlib.sha256(data).hexdigest()},indent=2)+'\n');p.unlink()
print('ARCHIVED',slug,flush=True)
