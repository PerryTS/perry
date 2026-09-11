from pathlib import Path
import gzip,hashlib,json,os,shlex,shutil,subprocess
w=Path(__file__).resolve().parent;bench=w.parents[1];host='perry@perry-macos.local';remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';prefix='.work/'+w.name+'/'
assert json.loads((w/'provenance.json').read_text())['source_commit']=='9415ce52ec36e72d643a70b0833240b407ea31d1'
files=[w/'run-korean-rotating.py']+sorted((w/'korean-fixtures').glob('*.json'))
expected=json.loads((w/'remote-stage-hashes.json').read_text())|{str(p.relative_to(bench)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
token='json-r43-korean-stage-'+str(os.getpid())
def ssh(code):return subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
ssh('from pathlib import Path;p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text('+repr(token)+')')
try:
 subprocess.run(['rsync','-aR',*[str(p.relative_to(bench)) for p in files],host+':'+remote+'/'],cwd=bench,check=True)
 ssh('from pathlib import Path;import hashlib;r=Path('+repr(remote)+');e='+repr(expected)+';assert all(hashlib.sha256((r/p).read_bytes()).hexdigest()==h for p,h in e.items());print("VERIFIED",len(e),"Korean and original assets")')
finally:ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()=='+repr(token)+';(p/"owner").unlink();p.rmdir()')
(w/'korean-stage-hashes.json').write_text(json.dumps(expected,indent=2)+'\n')
slug='quiet-'+w.name+'-korean-rotating'
cmd=['python3','with_lock.py','--','python3',prefix+'run-korean-rotating.py','--worker',prefix+'candidate-rotating-worker','--baseline-worker',prefix+'main-rotating-worker','--node','/opt/homebrew/bin/node','--bun','/Users/perry/.bun/bin/bun','--repeat','7','--source-commit','9415ce52ec36e72d643a70b0833240b407ea31d1','--results-dir','results/'+slug]
(w/'remote-korean-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with (w/'remote-korean.log').open('wb') as log:r=subprocess.run(['ssh',host,'cd '+shlex.quote(remote)+' && '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT)
# FIRST remote operation after every terminal window, including failures.
subprocess.run(['python3',str(w/'archive-results.py'),slug,'rotating','--allow-failed-window'],check=True)
d=bench/'results'/slug
for p in [Path(__file__),w/'run-korean-rotating.py',w/'remote-korean-command.json',w/'remote-korean.log',w/'korean-stage-hashes.json',w/'korean-plan.md']:shutil.copy2(p,d/p.name)
shutil.copy2(w/'korean-fixtures/manifest.json',d/'korean-manifest.json')
receipts=[]
for p in sorted((w/'korean-fixtures').glob('*.json')):
 if p.name=='manifest.json':continue
 raw=p.read_bytes();data=gzip.compress(raw,compresslevel=9,mtime=0);assert gzip.decompress(data)==raw
 q=d/'korean-fixtures'/(p.name+'.gz');q.parent.mkdir(exist_ok=True);q.write_bytes(data)
 receipts.append(dict(path=str(q.relative_to(d)),original_bytes=len(raw),original_sha256=hashlib.sha256(raw).hexdigest(),sha256=hashlib.sha256(data).hexdigest()))
(d/'korean-fixture-archive.json').write_text(json.dumps(receipts,indent=2)+'\n')
(d/'controller-exit.json').write_text(json.dumps(dict(exit_code=r.returncode),indent=2)+'\n')
print((w/'remote-korean.log').read_text(),flush=True)
raise SystemExit(r.returncode)
