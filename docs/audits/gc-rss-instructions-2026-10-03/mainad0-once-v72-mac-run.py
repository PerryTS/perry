import os,pathlib,subprocess,json,time,hashlib
B=pathlib.Path('/Users/amlug/projects/perry/secret-tests/scratchpad/rss-header-20261002')
W=pathlib.Path('/Users/amlug/projects/perry/rss-gc-runtime-mainad0-20261004')
inputs=json.loads((B/'mainad0-once-v72-inputs.json').read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
assert all(sha(W/n)==h for n,h in inputs.items())
for n in inputs:(W/n).touch()
env=os.environ|{'CARGO_TARGET_DIR':str(B/'mainad0-once-v72-mac-target'),'PERRY_RUNTIME_DIR':str(B/'mainad0-once-v72-mac-target/release'),'CARGO_BUILD_JOBS':'4','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1'}
cmd=['cargo','+nightly-2026-08-20','test','--locked','--release','-p','perry-runtime','--lib']
start=time.monotonic()
with (B/'mainad0-once-v72-mac-runtime.log').open('w') as f:
 p=subprocess.Popen(cmd,cwd=W,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait()
assert all(sha(W/n)==h for n,h in inputs.items())
(B/'mainad0-once-v72-mac-result.json').write_text(json.dumps(dict(command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,source_manifest_sha256=sha(B/'mainad0-once-v72-inputs.json'),raw_log_sha256=sha(B/'mainad0-once-v72-mac-runtime.log')),indent=2)+'\n')
raise SystemExit(rc)
