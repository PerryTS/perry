from pathlib import Path
import hashlib, json, os, subprocess

root = Path(__file__).resolve().parents[4]
work = Path(__file__).resolve().parent
os.chdir(root)
base = '53df2c671fff33b1f8f624432372c2401db8b1d0'
commit = subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain']), 'freeze tracked source before compilation'
clean = {k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
env = clean | {'PERRY_RUNTIME_DIR':str(work/'frozen-build')}
commands = []
for name in ['worker', 'rotating-worker', 'access-worker']:
    source = root/'benchmarks/json_performance'/(name+'.ts')
    obj = work/(name+'.o')
    binary = work/name
    cmd = [str(work/'frozen-build/perry'), 'compile', str(source),
           '--no-auto-optimize', '--no-cache', '--no-link', '--trace', 'llvm', '-o', str(obj)]
    commands.append(cmd)
    with (work/(name+'-compile.log')).open('wb') as log:
        subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
    cmd = ['cc',str(obj),str(work/'frozen-build/libperry_runtime.a'),'-lc',
           '-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)]
    commands.append(cmd)
    with (work/(name+'-link.log')).open('wb') as log:
        subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
    print('COMPILED',name,flush=True)
paths = [work/'frozen-build'/n for n in ['perry','libperry_runtime.a','libperry_stdlib.a']]
paths += [root/'benchmarks/json_performance'/(n+'.ts') for n in ['worker','rotating-worker','access-worker']]
paths += [work/n for n in ['worker','worker.o','rotating-worker','rotating-worker.o','access-worker','access-worker.o']]
meta = dict(source_commit=commit, base_commit=base, commands=commands,
            runtime_dir=str(work/'frozen-build'),
            files={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths})
(work/'provenance.json').write_text(json.dumps(meta,indent=2)+'\n')
(work/'source.patch').write_bytes(subprocess.check_output(['git','diff',base,commit]))
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==commit
