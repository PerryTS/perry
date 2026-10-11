#!/usr/bin/env python3
"""Regenerate native roots and reproduce shared findings on current main."""
import json
import os
from pathlib import Path
import subprocess

ROOT=Path(os.environ.get('KEYLISTS_ROOT', '/root/codex-lanes/cx-keylists'))
EVIDENCE=ROOT/'evidence2'
CPUS='0-23,32-55'
env=dict(os.environ, PERRY_BIN=str(ROOT/'target/release/perry'),
         PERRY_LLVM_OPT='/usr/lib/llvm-22/bin/opt', PERRY_MODULE_JOBS='1',
         PERRY_CODEGEN_UNIT_JOBS='1', CARGO_BUILD_JOBS='1',
         MIN_SOURCES='317', MAX_SKIPPED='0', PERRY_NO_CACHE='1')
# --no-link still resolves optional libraries after emitting IR. Refuse their
# optional Cargo builds in this private harness, rather than rebuilding a
# second feature graph behind the declared job limit. Codegen failures still
# fail the stock zero-skip corpus gate.
shim=ROOT/'tmp/native-no-build-bin'
shim.mkdir(exist_ok=True)
refusal=shim/'cargo'
refusal.write_text('#!/bin/sh\necho "keylists2: optional build refused in no-link corpus" >&2\nexit 1\n')
refusal.chmod(0o755)
env['PATH']=str(shim)+':'+env['PATH']

def run(argv, cwd, log, local=env, required=True):
    with (EVIDENCE/log).open('w') as output:
        r=subprocess.run(['taskset','-c',CPUS,*argv],cwd=cwd,env=local,
                         stdout=output,stderr=subprocess.STDOUT)
    if required and r.returncode:
        raise RuntimeError((argv,r.returncode,log))
    return r.returncode

corpus=EVIDENCE/'native-current236'
run(['bash','scripts/gc_root_dominance_corpus.sh',str(corpus),'--lowering','native'],
    ROOT/'src','native-current236-corpus.log')
gate=run(['python3','lane-keylists/root-check-indexed.py',str(corpus),
          '--statepoints','--moving-only','--max-unrooted','0','--max-stale','0',
          '--min-files','361','--min-funcs','9938','--min-statepoints','100463',
          '--min-live-bundles','71189','--min-relocates','320312',
          '--seeded-violations','40'],ROOT/'src','native-current236-moving-gate.log',
         required=False)

base=ROOT/'main-current'
local=dict(env,CARGO_TARGET_DIR=str(base/'target'),
           PERRY_RUNTIME_DIR=str(base/'target/release'),
           PERRY_WORKSPACE_ROOT=str(base/'src'), PERRY_GC_MOVING_LOOP_POLLS='1',
           PERRY_NO_AUTO_OPTIMIZE='1')
controls=EVIDENCE/'native-current236-main-controls'
controls.mkdir(exist_ok=True)
passes=subprocess.check_output(['python3','scripts/read_statepoint_rewrite_passes.py'],
                               cwd=base/'src',env=local,text=True).strip()
for name in ['test_gap_array_append_compact','test_gap_array_record_stack_custody']:
    trace=base/'src/.perry-trace/llvm'
    if trace.exists():
        for path in trace.glob('*.ll'):path.unlink()
    run([str(base/'target/release/perry'),'compile','test-files/'+name+'.ts',
         '--no-auto-optimize','--no-link','--trace','llvm','-o',str(controls/(name+'.o'))],
        base/'src',name+'-current236-main-compile.log',local)
    modules=list(trace.glob('*.ll'))
    assert modules, name
    for path in modules:
        subprocess.run(['taskset','-c',CPUS,local['PERRY_LLVM_OPT'],
                        '-passes='+passes,'-S',str(path),'-o',
                        str(controls/(name+'__'+path.name))],env=local,check=True)
control=run(['python3','scripts/gc_root_dominance_check.py',str(controls),
             '--statepoints','--moving-only','--max-unrooted','0','--max-stale','0',
             '--min-files','2','--min-funcs','1','--min-statepoints','1',
             '--min-live-bundles','1','--min-relocates','1'],
            base/'src','native-current236-main-original-gate.log',local,required=False)
(EVIDENCE/'native-current236-status.json').write_text(json.dumps({
    'corpus_gate_rc':gate,'main_original_control_rc':control,
    'zero_budgets_retained':True,'scope':'native root census and baseline controls'},indent=2)+'\n')
print('Native root round finished',gate,control,flush=True)
