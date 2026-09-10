from pathlib import Path
import subprocess,os,json
w=Path(__file__).resolve().parent;root=w.parents[3]
cmd=['cargo','test','--release','-p','perry-runtime','--lib','json_cached_array']
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}|{'RUST_TEST_THREADS':'1'}
with (w/'focused-unit.log').open('wb') as log:r=subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
(w/'focused-unit-result.json').write_text(json.dumps({'command':cmd,'env':{'RUST_TEST_THREADS':'1'},'exit_code':r.returncode},indent=2)+'\n')
print('Focused units terminal',r.returncode,flush=True)
print((w/'focused-unit.log').read_text()[-4000:],flush=True)
raise SystemExit(r.returncode)
