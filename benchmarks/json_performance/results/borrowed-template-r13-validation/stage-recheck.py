from pathlib import Path
import hashlib, json, os, shlex, subprocess

w = Path(__file__).resolve().parent
bench = w.parents[1]
host = 'perry@perry-macos.local'
remote = '/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
expected = json.loads((w / 'remote-stage-hashes.json').read_text())
files = [str((w / name).relative_to(bench)) for name in ['run-rotating-recheck.py', 'recheck-counts.json']]
expected.update({p: hashlib.sha256((bench / p).read_bytes()).hexdigest() for p in files})
assert all(hashlib.sha256((bench / p).read_bytes()).hexdigest() == h for p, h in expected.items())
token = 'json-r13-recheck-stage-' + str(os.getpid())
def ssh(code):
    return subprocess.run(['ssh', host, 'python3 -c ' + shlex.quote(code)], check=True)
ssh('from pathlib import Path;p=Path.home()/"bench.lock";p.mkdir();(p/"owner").write_text(' + repr(token) + ')')
try:
    subprocess.run(['rsync', '-aR'] + files + [host + ':' + remote + '/'], cwd=bench, check=True)
    ssh('from pathlib import Path;import hashlib;root=Path(' + repr(remote) + ');expected=' + repr(expected) + ';actual={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in expected};assert actual==expected;print("VERIFIED",len(expected),"remote hashes")')
    (w / 'remote-recheck-stage-hashes.json').write_text(json.dumps(expected, indent=2) + '\n')
finally:
    ssh('from pathlib import Path;p=Path.home()/"bench.lock";assert(p/"owner").read_text()==' + repr(token) + ';(p/"owner").unlink();p.rmdir()')
