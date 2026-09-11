from pathlib import Path
import gzip,hashlib,json,os,shlex,shutil,subprocess
w=Path(__file__).resolve().parent;bench=w.parents[1];remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';host='perry@perry-macos.local';prefix='.work/'+w.name+'/'
names=['with_lock.py','run_dispatch_focus.py']+[prefix+n for n in ['run-access-profiles.py','profile-cases.json','main-access-worker','candidate-access-worker','main-workers-provenance.json','candidate-workers-provenance.json','main-build-provenance.json','build-provenance.json','main-reuse-provenance.json','reference-artifacts.json','provenance.json','source.patch']]+['.work/fixtures/'+f+'.json' for f in ['records_array_16k','records_array_1m']]
expected={p:hashlib.sha256((bench/p).read_bytes()).hexdigest() for p in names};token='json-r24-access-profiles-'+str(os.getpid())
def ssh(code):subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
ssh('from pathlib import Path;p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text('+repr(token)+')')
try:
 subprocess.run(['rsync','-aR',*names,host+':'+remote+'/'],cwd=bench,check=True)
 ssh('from pathlib import Path;import hashlib;root=Path('+repr(remote)+');expected='+repr(expected)+';assert all(hashlib.sha256((root/p).read_bytes()).hexdigest()==h for p,h in expected.items())')
finally:ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()=='+repr(token)+';(p/"owner").unlink();p.rmdir()')
(w/'access-profile-stage-hashes.json').write_text(json.dumps(expected,indent=2)+'\n')
slug='quiet-'+w.name+'-access-profiles';cmd=['python3','with_lock.py','--','python3',prefix+'run-access-profiles.py','--results-dir','results/'+slug]
(w/'access-profile-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with (w/'access-profile-remote.log').open('wb') as log:r=subprocess.run(['ssh',host,'cd '+shlex.quote(remote)+' && '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT)
# FIRST remote operation after terminal window: preserve its window files.
code='''from pathlib import Path
import json,shutil
root=Path(REMOTE);d=root/'results'/SLUG
window=json.loads((root/'results/window-custom.json').read_text());assert window.get('finished_utc') and 'results/'+SLUG in window['command']
d.mkdir(parents=True,exist_ok=True)
for p,n in [('results/window-custom.json','window.json'),('custom.log','controller.log'),('results/processes-before.txt','processes-before.txt'),('results/processes-after.txt','processes-after.txt'),('with_lock.py','with_lock.py'),('run_dispatch_focus.py','run_dispatch_focus.py')]:shutil.copy2(root/p,d/n)
for n in ['run-access-profiles.py','profile-cases.json','main-workers-provenance.json','candidate-workers-provenance.json','main-build-provenance.json','build-provenance.json','main-reuse-provenance.json','reference-artifacts.json','provenance.json','source.patch']:shutil.copy2(root/PREFIX/n,d/n)
'''.replace('REMOTE',repr(remote)).replace('SLUG',repr(slug)).replace('PREFIX',repr(prefix))
ssh(code)
subprocess.run(['rsync','-a',host+':'+remote+'/results/'+slug+'/',str(bench/'results'/slug)+'/'],check=True)
d=bench/'results'/slug
patch=d/'source.patch';raw=patch.read_bytes();data=gzip.compress(raw,compresslevel=9,mtime=0);assert gzip.decompress(data)==raw
patch.with_suffix('.patch.gz').write_bytes(data);patch.with_suffix('.patch.json').write_text(json.dumps({'original_bytes':len(raw),'original_sha256':hashlib.sha256(raw).hexdigest(),'gzip_sha256':hashlib.sha256(data).hexdigest()},indent=2)+'\n');patch.unlink()
for p in [Path(__file__),w/'access-profile-command.json',w/'access-profile-stage-hashes.json',w/'access-profile-remote.log']:shutil.copy2(p,d/p.name)
(d/'controller-exit.json').write_text(json.dumps({'exit_code':r.returncode},indent=2)+'\n')
print('ARCHIVED',slug,flush=True);print((w/'access-profile-remote.log').read_text(),flush=True)
raise SystemExit(r.returncode)
