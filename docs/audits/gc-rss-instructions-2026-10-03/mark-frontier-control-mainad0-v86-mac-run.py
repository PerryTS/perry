import pathlib,json,hashlib,subprocess,os,time,re
B=pathlib.Path(__file__).resolve().parent;W=pathlib.Path('/Users/amlug/projects/perry/rss-gc-mark-frontier-control-mainad0-20261004');V='mark-frontier-control-mainad0-v86';T=B/(V+'-mac-target')
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
inputs=json.loads((B/'mark-frontier-mainad0-v81-inputs.json').read_text())
for n in inputs:inputs[n]=sha(W/n)
for n in ['crates/perry-runtime/src/gc/trace.rs','crates/perry-runtime/src/gc/cycle.rs']:
 original=subprocess.check_output(['git','show','0aa483aa91ab901583002ec0174a50884b60ad81:'+n],cwd=W);assert hashlib.sha256(original).hexdigest()==inputs[n]
assert len(inputs)==5015
(B/(V+'-inputs.json')).write_text(json.dumps(inputs,indent=2)+'\n')
for n in inputs:(W/n).touch()
env=os.environ|dict(CARGO_TARGET_DIR=str(T),PERRY_RUNTIME_DIR=str(T/'release'),PERRY_WORKSPACE_ROOT=str(W),CARGO_BUILD_JOBS='4',CARGO_PROFILE_RELEASE_CODEGEN_UNITS='16',CARGO_INCREMENTAL='0',RUST_TEST_THREADS='1')
cmd=['cargo','+nightly-2026-08-20','test','--locked','--release','-p','perry-runtime','mark_frontier']
assert not (B/(V+'-mac-result.json')).exists() and not (B/(V+'-mac-runtime.log')).exists()
start=time.monotonic()
with (B/(V+'-mac-runtime.log')).open('w') as f:
 p=subprocess.Popen(cmd,cwd=W,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait()
r=dict(command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,source_manifest_sha256=sha(B/(V+'-inputs.json')),raw_log_sha256=sha(B/(V+'-mac-runtime.log')),scope='Negative control: original production queue and cycle state restored exactly. Two private frontier tests are required to fail their retained-capacity assertion.')
(B/(V+'-mac-result.json')).write_text(json.dumps(r,indent=2)+'\n')
assert all(sha(W/n)==h for n,h in inputs.items())
log=(B/(V+'-mac-runtime.log')).read_text();assert rc==101 and 'test result: FAILED. 0 passed; 2 failed;' in log
assert log.count('processed entries must not accumulate')==2
bins=[pathlib.Path(p) for p in re.findall(r'Running unittests .*?\(([^)]+)\)',log)];assert len(bins)==1
proof=dict(result=r,source_count=len(inputs),test_binary_hashes={str(p):sha(p) for p in bins},both_tests_detect_original_queue_retention=True,scope=r['scope'])
(B/(V+'-mac-verification.json')).write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof))
