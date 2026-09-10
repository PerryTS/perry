from pathlib import Path
import hashlib, json, os, re, subprocess, sys

root = Path(__file__).resolve().parents[4]
work = Path(__file__).resolve().parent
env = {k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
env |= {'PERRY_RUNTIME_DIR':str(root/'target/release'), 'PERRY_WORKSPACE_ROOT':str(root),
        'PERRY_GC_MOVING_LOOP_POLLS':'1', 'PERRY_INLINE_SHADOW_SLOT':'0', 'PERRY_NO_AUTO_OPTIMIZE':'1'}
passes = subprocess.check_output(['python3',str(root/'scripts/read_statepoint_rewrite_passes.py')],cwd=root,text=True).strip()
sources = [root/'test-files/test_json_scalar_projection.ts',
           root/'benchmarks/json_performance/access-worker.ts',
           root/'benchmarks/json_performance/worker.ts']
counts = []
checks = []
for mode, rs4gc in ([('shadow','0')] if '--shadow-only' in sys.argv else [('native','1'), ('shadow','0')]):
    output = work/('ir-'+mode)
    output.mkdir(exist_ok=False)
    for source in sources:
        scratch = output/source.stem
        scratch.mkdir()
        cmd = [str(root/'target/release/perry'), 'compile', str(source), '--no-auto-optimize',
               '--no-cache', '--no-link', '--trace', 'llvm', '-o', str(scratch/'worker.o')]
        with (scratch/'compile.log').open('wb') as log:
            subprocess.run(cmd,cwd=scratch,env=env|{'PERRY_RS4GC':rs4gc},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
        files = list((scratch/'.perry-trace/llvm').glob('*.ll'))
        assert files, scratch
        calls = 0
        hints = 0
        for index, ll in enumerate(files):
            original = ll.read_text()
            hints += len(re.findall(r"\bcall\s+i1\s+@llvm\.expect\.i1\(", original))
            calls += len(re.findall(r'\bcall\s+double\s+@js_json_lazy_index_scalar\(',original))
            dest = output/(source.stem+'-'+str(index)+'.ll')
            if mode == 'native':
                with (scratch/('rewrite-'+str(index)+'.log')).open('wb') as log:
                    subprocess.run(['/opt/homebrew/opt/llvm/bin/opt','-passes='+passes,'-S',str(ll),'-o',str(dest)],
                                   stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
                for line in dest.read_text().splitlines():
                    assert not ('gc.statepoint' in line and '@js_json_lazy_index_scalar' in line), line
            else:
                dest.write_bytes(ll.read_bytes())
        assert calls > 0 and hints == calls, (source,mode,calls,hints,'probe/hint not emitted')
        counts.append(dict(source=str(source.relative_to(root)),lowering=mode,probe_calls=calls,expect_calls=hints))
        print('LIVE PROBE',source.name,mode,calls,flush=True)
    checker = ['python3',str(root/'scripts/gc_root_dominance_check.py')]
    paths = [str(p) for p in output.glob('*.ll')]
    variants = [['--statepoints','--min-files','3','--min-statepoints','1','--min-live-bundles','1','--min-relocates','1']] if mode=='native' else [[],['--unrooted-allocas']]
    for index, flags in enumerate(variants):
        with (output/('check-'+str(index)+'.log')).open('wb') as log:
            result=subprocess.run(checker+flags+paths,cwd=root,stdout=log,stderr=subprocess.STDOUT)
        checks.append(dict(mode=mode, scope='all', variant=index, exit_code=result.returncode))
        print('ROOTS',mode,index,result.returncode,flush=True)
    if mode == 'native':
        workers = [str(p) for p in output.glob('*.ll') if not p.name.startswith('test_json_scalar_projection')]
        with (output/'workers-check.log').open('wb') as log:
            result = subprocess.run(checker+['--statepoints','--min-files','2','--min-statepoints','1','--min-live-bundles','1','--min-relocates','1']+workers,
                                    cwd=root,stdout=log,stderr=subprocess.STDOUT)
        checks.append(dict(mode=mode, scope='workers', exit_code=result.returncode))
        print('ROOTS workers',result.returncode,flush=True)
(work/('shadow-codegen-comparison.json' if '--shadow-only' in sys.argv else 'codegen-comparison.json')).write_text(json.dumps(dict(probe_counts=counts,checks=checks,statepoint_passes=passes,
    source_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()),indent=2)+'\n')

sys.exit(0 if all(c["exit_code"] == 0 for c in checks) else 1)
