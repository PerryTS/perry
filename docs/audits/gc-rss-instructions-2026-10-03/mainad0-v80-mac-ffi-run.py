from pathlib import Path
import os,json,hashlib,subprocess,time,re
B=Path('/Users/amlug/projects/perry/secret-tests/scratchpad/rss-header-20261002')
W=Path('/Users/amlug/projects/perry/rss-gc-runtime-mainad0-20261004')
V='mainad0-v80-mac-ffi'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
inputs=json.loads((B/'mainad0-once-v72-inputs.json').read_text())
assert len(inputs)==5014 and all(sha(W/n)==h for n,h in inputs.items())
for n in inputs: (W/n).touch()
T=B/'mainad0-once-v72-mac-target'
env=os.environ|dict(CARGO_TARGET_DIR=str(T),PERRY_RUNTIME_DIR=str(T/'release'),PERRY_WORKSPACE_ROOT=str(W),CARGO_BUILD_JOBS='4',CARGO_PROFILE_RELEASE_CODEGEN_UNITS='16',CARGO_INCREMENTAL='0',RUST_TEST_THREADS='1')
cmd=['cargo','+nightly-2026-08-20','test','--locked','--release','-p','perry-ffi','-p','perry-ext-events']
assert not (B/(V+'-result.json')).exists() and not (B/(V+'.log')).exists()
start=time.monotonic()
with (B/(V+'.log')).open('w') as f:
    p=subprocess.Popen(cmd,cwd=W,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True)
    rc=p.wait()
assert all(sha(W/n)==h for n,h in inputs.items())
result=dict(command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,source_manifest_sha256=sha(B/'mainad0-once-v72-inputs.json'),raw_log_sha256=sha(B/(V+'.log')),source_equivalent_to='0aa483aa91ab901583002ec0174a50884b60ad81')
(B/(V+'-result.json')).write_text(json.dumps(result,indent=2)+'\n')
assert rc==0
text=(B/(V+'.log')).read_text()
assert 'Compiling perry-runtime ' in text
counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; 0 measured; 0 filtered out;',text)
assert counts==[('15','0','0'),('75','0','0'),('0','0','0'),('5','0','13')],counts
(B/(V+'-verification.json')).write_text(json.dumps(dict(source_count=5014,result=result,suites=counts,scope='Pinned mainad0 integration: Mac FFI/events/doc tests. Source/log/command verified; independent verification remains required.'),indent=2)+'\n')
