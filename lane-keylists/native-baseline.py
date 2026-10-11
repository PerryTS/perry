#!/usr/bin/env python3
"""Match the current compiler's native population before its zero-budget gate."""
import hashlib
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

ROOT=Path(os.environ.get('KEYLISTS_ROOT', '/root/codex-lanes/cx-keylists'))
p=argparse.ArgumentParser()
p.add_argument('--reuse-corpus',action='store_true')
args=p.parse_args()
E=ROOT/'evidence2'
BASE=ROOT/'main-current'
NATIVE_TARGET=ROOT/'main-native-target'
assert 'Finished ' in (ROOT/'keylists2-current236-main-native-net-build.log').read_text()
env=dict(os.environ,CARGO_TARGET_DIR=str(NATIVE_TARGET),
    PERRY_RUNTIME_DIR=str(NATIVE_TARGET/'release'),PERRY_WORKSPACE_ROOT=str(BASE/'src'),
    PERRY_BIN=str(BASE/'target/release/perry'),PERRY_LLVM_OPT='/usr/lib/llvm-22/bin/opt',
    PERRY_MODULE_JOBS='1',PERRY_CODEGEN_UNIT_JOBS='1',CARGO_BUILD_JOBS='1',
    MIN_SOURCES='317',MAX_SKIPPED='0',PERRY_NO_CACHE='1',
    PATH=str(ROOT/'tmp/native-no-build-bin')+':'+os.environ['PATH'])
corpus=E/'native-current236-main'
if not args.reuse_corpus:
    with (E/'native-current236-main-corpus.log').open('w') as output:
        subprocess.run(['taskset','-c','0-23,32-55','bash','scripts/gc_root_dominance_corpus.sh',
                        str(corpus),'--lowering','native'],cwd=BASE/'src',env=env,
                       stdout=output,stderr=subprocess.STDOUT,check=True)
assert '317/317 sources compiled, 0 skipped, 361 .ll files' in (E/'native-current236-main-corpus.log').read_text()

def population(path, prefix):
    sums=dict(files=0,funcs=0,statepoints=0,live_bundles=0,relocates=0)
    hashes={}
    checker=ROOT/'src/scripts/gc_root_dominance_check.py'
    ns={'__name__':'native_census_reader','__file__':str(checker)}
    exec(compile(checker.read_text(),str(checker),'exec'),ns)
    for file in sorted(path.glob('*.ll')):
        text=file.read_text()
        functions=ns['parse_file'](str(file))
        points,live,relocates,_roots,unreadable=ns['statepoint_corpus_stats']([(file.name,functions)])
        assert not unreadable,(file,unreadable)
        sums['files']+=1
        sums['funcs']+=len(functions)
        sums['statepoints']+=points
        sums['live_bundles']+=live
        sums['relocates']+=relocates
        hashes[file.name]=hashlib.sha256(text.replace(str(prefix),'TREE').encode()).hexdigest()
    return sums,hashes

base,base_hash=population(corpus,BASE/'src')
head,head_hash=population(E/'native-current236',ROOT/'src')
assert base==head,(base,head)
assert base['files']==361 and base['funcs']==9938,base
assert all(base[k]>0 for k in base),base
(E/'native-current236-cohort.json').write_text(json.dumps({
    'main':base,'head':head,'normalized_ir_identical':base_hash==head_hash,
    'scope':'current-main compiler cohort; zero hazard budgets unchanged'},indent=2)+'\n')
flags=['--statepoints','--moving-only','--max-unrooted','0','--max-stale','0',
       '--min-files',str(base['files']),'--min-funcs',str(base['funcs']),
       '--min-statepoints',str(base['statepoints']),
       '--min-live-bundles',str(base['live_bundles']),
       '--min-relocates',str(base['relocates'])]
statuses={}
for name,path in [('main',corpus),('head',E/'native-current236')]:
    extra=['--seeded-violations','40'] if name=='head' else []
    with (E/f'native-current236-cohort-{name}-gate.log').open('w') as output:
        result=subprocess.run(['taskset','-c','0-23,32-55','python3',
            str(ROOT/'src/lane-keylists/root-check-indexed.py'),str(path),*flags,*extra],
            cwd=ROOT/'src',env=env,stdout=output,stderr=subprocess.STDOUT)
    statuses[name]=result.returncode
(E/'native-current236-cohort-status.json').write_text(json.dumps(statuses)+'\n')
print('Matched current native cohort',base,statuses,flush=True)
