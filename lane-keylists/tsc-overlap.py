#!/usr/bin/env python3
"""Compile fresh tsc objects with two jobs while runtime graphs finish."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT=Path(os.environ['KEYLISTS_ROOT'])
BASE=ROOT/'main-current'
proof=ROOT/'tmp/early-tsc.json'
running=ROOT/'tmp/early-tsc.running'
assert not proof.exists() and not running.exists()
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
compiler=digest(BASE/'target/release/perry')
inputs=[ROOT/'drivers/tscwork.ts']
package=ROOT/'drivers/node_modules/typescript'
assert package.is_dir(),package
inputs += [p for p in package.rglob('*') if p.is_file() and p.suffix in ['.js','.ts','.json']]
hashes={str(p):digest(p) for p in inputs}
running.write_text('two frontend jobs reserved\n')
try:
    env=dict(os.environ,KEYLISTS_MAIN='main-current',KEYLISTS_EVIDENCE='evidence2',
             KEYLISTS_MODULE_JOBS='2',KEYLISTS_CODEGEN_JOBS='1',KEYLISTS_EARLY_TSC='1')
    name=os.environ.get('KEYLISTS_ROUND_NAME', 'current16')
    with (ROOT/f'keylists2-{name}-tsc-frontend.log').open('w') as output:
        subprocess.run(['taskset','-c','0-23,32-55','python3','run.py','compile',
                        'main','--only','tscwork'],cwd=ROOT/'src/lane-keylists',
                       env=env,stdout=output,stderr=subprocess.STDOUT,check=True)
    status=json.loads((ROOT/'evidence2/main/status.json').read_text())['tscwork']
    assert status['equal'] and status['compile_rc']==status['node_rc']==status['run_rc']==0,status
    assert compiler==digest(BASE/'target/release/perry')
    assert all(digest(Path(p))==h for p,h in hashes.items())
    proof.write_text(json.dumps(dict(compiler=compiler,inputs=hashes,status=status))+'\n')
finally:
    running.unlink()
print('Verified fresh tsc frontend ready',flush=True)
