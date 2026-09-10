from pathlib import Path
import hashlib
import json
import os
import shlex
import subprocess

w = Path(__file__).resolve().parent
bench = w.parents[1]
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
host = 'perry@perry-macos.local'
names = [str((w / n).relative_to(bench)) for n in ['run-profiles.py', 'profile-cases.json']]
expected = json.loads((w / 'remote-stage-hashes.json').read_text())
expected.update({p: hashlib.sha256((bench / p).read_bytes()).hexdigest() for p in names})
assert len(expected) == 105
token = 'json-r18-profiles-stage-' + str(os.getpid())
def ssh(code):
    subprocess.run(['ssh', host, 'python3 -c ' + shlex.quote(code)], check=True)
ssh('from pathlib import Path;p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text(' + repr(token) + ')')
try:
    subprocess.run(['rsync', '-aR', *names, host + ':' + remote + '/'], cwd=bench, check=True)
    ssh('from pathlib import Path;import hashlib;root=Path(' + repr(remote) + ');expected=' + repr(expected) + ';assert all(hashlib.sha256((root/p).read_bytes()).hexdigest()==h for p,h in expected.items());print("VERIFIED",len(expected),"profile input hashes")')
    (w / 'remote-profile-stage-hashes.json').write_text(json.dumps(expected, indent=2) + '\n')
finally:
    ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()==' + repr(token) + ';(p/"owner").unlink();p.rmdir()')
