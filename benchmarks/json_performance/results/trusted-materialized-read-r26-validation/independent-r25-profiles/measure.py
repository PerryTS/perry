from pathlib import Path
import gzip,hashlib,json,os,shlex,shutil,subprocess
w=Path(__file__).resolve().parent;bench=w.parents[1];remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';host='perry@perry-macos.local';prefix='.work/'+w.name+'/'
names=['with_lock.py','run_dispatch_focus.py']+[prefix+n for n in ['run-parse-profiles.py','profile-cases.json','r25-rotating-worker','workers-provenance.json','build-provenance.json','source.patch','rotating-worker.ts','rotating-worker.js']]+['.work/rotating/'+f+'.'+str(i)+'.json' for f in ['small_record','long_string_1m','unicode_1m'] for i in range(8)]
expected={p:hashlib.sha256((bench/p).read_bytes()).hexdigest() for p in names};token='json-parse-r25-profiles-'+str(os.getpid())
assert json.loads((w/'build-provenance.json').read_text())['source_commit']=='01f2878dad8efc92e394c49b88fe0800b871f593'
proof=next(r for r in json.loads((w/'workers-provenance.json').read_text()) if r['worker']=='rotating-worker')
assert expected[prefix+'r25-rotating-worker']==next(h for p,h in proof['files'].items() if p.endswith('/candidate-rotating-worker'))
def ssh(code):subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
ssh('from pathlib import Path;p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text('+repr(token)+')')
try:
 subprocess.run(['rsync','-aR',*names,host+':'+remote+'/'],cwd=bench,check=True)
 ssh('from pathlib import Path;import hashlib;root=Path('+repr(remote)+');expected='+repr(expected)+';assert all(hashlib.sha256((root/p).read_bytes()).hexdigest()==h for p,h in expected.items());print("VERIFIED",len(expected),"profile assets")')
finally:ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()=='+repr(token)+';(p/"owner").unlink();p.rmdir()')
(w/'stage-hashes.json').write_text(json.dumps(expected,indent=2)+'\n')
slug='quiet-'+w.name;cmd=['python3','with_lock.py','--','python3',prefix+'run-parse-profiles.py','--results-dir','results/'+slug]
(w/'remote-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with (w/'remote.log').open('wb') as log:r=subprocess.run(['ssh',host,'cd '+shlex.quote(remote)+' && '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT)
# FIRST remote operation after every terminal window, including a failed diagnostic.
code='''from pathlib import Path
import json,shutil
root=Path(REMOTE);d=root/'results'/SLUG
window=json.loads((root/'results/window-custom.json').read_text());assert window.get('finished_utc') and 'results/'+SLUG in window['command']
d.mkdir(parents=True,exist_ok=True)
for p,n in [('results/window-custom.json','window.json'),('custom.log','controller.log'),('results/processes-before.txt','processes-before.txt'),('results/processes-after.txt','processes-after.txt'),('with_lock.py','with_lock.py'),('run_dispatch_focus.py','run_dispatch_focus.py')]:shutil.copy2(root/p,d/n)
for n in ['run-parse-profiles.py','profile-cases.json','workers-provenance.json','build-provenance.json','source.patch','rotating-worker.ts','rotating-worker.js']:shutil.copy2(root/PREFIX/n,d/n)
'''.replace('REMOTE',repr(remote)).replace('SLUG',repr(slug)).replace('PREFIX',repr(prefix))
ssh(code)
# Archive locally under ignored WORK while the independent R26 build requires a clean source tree.
d=w/'archive';subprocess.run(['rsync','-a',host+':'+remote+'/results/'+slug+'/',str(d)+'/'],check=True)
p=d/'source.patch';raw=p.read_bytes();data=gzip.compress(raw,compresslevel=9,mtime=0);assert gzip.decompress(data)==raw
p.with_suffix('.patch.gz').write_bytes(data);p.with_suffix('.patch.json').write_text(json.dumps({'original_bytes':len(raw),'original_sha256':hashlib.sha256(raw).hexdigest(),'gzip_sha256':hashlib.sha256(data).hexdigest()},indent=2)+'\n');p.unlink()
for p in [Path(__file__),w/'remote-command.json',w/'stage-hashes.json',w/'remote.log']:shutil.copy2(p,d/p.name)
(d/'controller-exit.json').write_text(json.dumps({'exit_code':r.returncode},indent=2)+'\n')
print('ARCHIVED',slug,'to ignored WORK/archive',flush=True);print((w/'remote.log').read_text(),flush=True)
raise SystemExit(r.returncode)
