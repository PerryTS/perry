from pathlib import Path
import os,json,hashlib,subprocess,time,re
B=Path('/Users/amlug/projects/perry/secret-tests/scratchpad/rss-header-20261002')
W=Path('/Users/amlug/projects/perry/rss-gc-runtime-latest2026-20261003')
V='once-advice-main07-v65-mac-ffi'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
inputs=json.loads((B/'entry-once-main07-v45-inputs.json').read_text())
assert len(inputs)==5011 and all(sha(W/n)==h for n,h in inputs.items())
for n in inputs: (W/n).touch()
T=B/'entry-once-main07-v45-mac-target'
env=os.environ|dict(CARGO_TARGET_DIR=str(T),PERRY_RUNTIME_DIR=str(T/'release'),PERRY_WORKSPACE_ROOT=str(W),CARGO_BUILD_JOBS='4',CARGO_PROFILE_RELEASE_CODEGEN_UNITS='16',CARGO_INCREMENTAL='0',RUST_TEST_THREADS='1')
cmd=['cargo','+nightly-2026-08-20','test','--locked','--release','-p','perry-ffi','-p','perry-ext-events']
start=time.monotonic()
with (B/(V+'.log')).open('w') as f:
    p=subprocess.Popen(cmd,cwd=W,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
    rc=p.wait()
assert all(sha(W/n)==h for n,h in inputs.items())
result=dict(command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,source_manifest_sha256=sha(B/'entry-once-main07-v45-inputs.json'),raw_log_sha256=sha(B/(V+'.log')),source_equivalent_to='8e3013733da37ddc66998ee51b9bd1109a9175ec')
(B/(V+'-result.json')).write_text(json.dumps(result,indent=2)+'\n')
assert rc==0
text=(B/(V+'.log')).read_text()
assert 'Compiling perry-runtime ' in text
counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; 0 measured; 0 filtered out;',text)
assert counts==[('75','0','0'),('15','0','0'),('5','0','13'),('0','0','0')],counts
(B/(V+'-verification.json')).write_text(json.dumps(dict(source_count=5011,result=result,suites=counts,scope='Exact once-advice candidate adopted into draft PR, Mac FFI/events and doc tests. Source/log/command verified.'),indent=2)+'\n')
