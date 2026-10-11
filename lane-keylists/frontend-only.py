#!/usr/bin/env python3
"""Complete fresh baseline objects without measuring an obsolete candidate."""
import json
import os
from pathlib import Path
import subprocess

ROOT=Path(os.environ.get('KEYLISTS_ROOT', '/root/codex-lanes/cx-keylists'))
for arm in ['main','head']:
    for suffix in ['build','stream']:
        log=ROOT/f'keylists2-current236-final-{arm}-{suffix}.log'
        assert 'Finished ' in log.read_text(), log
    log=ROOT/f'keylists2-current236-final-obs-{arm}-stream.log'
    assert 'Finished ' in log.read_text(), log
env=dict(os.environ,KEYLISTS_MAIN='main-current',KEYLISTS_EVIDENCE='evidence2',
         KEYLISTS_MODULE_JOBS='6',KEYLISTS_CODEGEN_JOBS='6')
with (ROOT/'keylists2-current236-frontend-only.log').open('w') as output:
    subprocess.run(['taskset','-c','0-23,32-55','python3','run.py','compile','main'],
                   cwd=ROOT/'src/lane-keylists',env=env,stdout=output,
                   stderr=subprocess.STDOUT,check=True)
status=json.loads((ROOT/'evidence2/main/status.json').read_text())
assert len(status)==18 and all(r.get('equal') and r.get('compile_rc')==0
    and r.get('node_rc')==0 and r.get('run_rc')==0 for r in status.values())
for name in status:
    assert (ROOT/'evidence2/main'/(name+'.link.json')).exists(), name
(ROOT/'evidence2/completed-current236-frontends.json').write_text(json.dumps({
    'complete':True,'scope':'fresh baseline frontend objects, not acceptance measurements',
    'base':'236adad78d03ff2bece0d89e2702ccd0a4700e6a','programs':18})+'\n')
print('Current-main frontends ready, 18/18 Node matches',flush=True)
