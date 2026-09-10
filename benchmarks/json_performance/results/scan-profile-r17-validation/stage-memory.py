from pathlib import Path
import hashlib,json,os,shlex,subprocess
w=Path(__file__).resolve().parent;bench=w.parents[1];host='perry@perry-macos.local';remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';expected=json.loads((w/'remote-stage-hashes.json').read_text());files=[str((w/n).relative_to(bench)) for n in ['run-memory.py','memory-cases.json','memory-worker','memory-worker.ts','memory-worker-provenance.json']]
for p in files:expected[p]=hashlib.sha256((bench/p).read_bytes()).hexdigest()
assert all(hashlib.sha256((bench/p).read_bytes()).hexdigest()==h for p,h in expected.items())
token='json-r17-memory-stage-'+str(os.getpid())
def ssh(code):subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
ssh('from pathlib import Path;p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text('+repr(token)+')')
try:
 subprocess.run(['rsync','-aR']+files+[host+':'+remote+'/'],cwd=bench,check=True)
 ssh('from pathlib import Path;import hashlib;r=Path('+repr(remote)+');e='+repr(expected)+';assert {p:hashlib.sha256((r/p).read_bytes()).hexdigest() for p in e}==e;print("VERIFIED",len(e),"memory diagnostic input hashes")')
 (w/'remote-memory-stage-hashes.json').write_text(json.dumps(expected,indent=2)+'\n')
finally:ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()=='+repr(token)+';(p/"owner").unlink();p.rmdir()')
