"""Separate test-only overlay; does not mutate running app source/products."""
import pathlib,json,subprocess,shutil,hashlib,os,time
B=pathlib.Path('/root/rss-header-20261002');P=B/'primary-idle-eden-v30';R=B/'primary-idle-eden-v30-residency';R.mkdir(exist_ok=False)
S=R/'source';T=R/'target';E=R/'export';E.mkdir()
shutil.copytree(P/'source',S,ignore=shutil.ignore_patterns('target','node_modules','.git'))
subprocess.run(['cp','-a','--reflink=auto',P/'target',T],check=True)
rel='crates/perry-runtime/src/arena/block/reuse_window_tests.rs';shutil.copy2(B/'private-idle-eden-v30-residency-tests.rs',S/rel)
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
inputs=json.loads((P/'export/source-inputs.json').read_text());assert all(sha(S/n)==h for n,h in inputs.items() if n!=rel)
inputs[rel]=sha(S/rel);(E/'source-inputs.json').write_text(json.dumps(inputs,indent=2)+'\n')
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'CARGO_BUILD_JOBS':'8','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1','CARGO_TARGET_DIR':str(T),'PERRY_RUNTIME_DIR':str(T/'release'),'PERRY_WORKSPACE_ROOT':str(S)}
cmd=['cargo','test','--locked','--release','-p','perry-runtime','--lib','idle_eden_pages_leave_rss_without_releasing_recent_or_reused_blocks','--','--nocapture']
start=time.monotonic()
with (E/'test.log').open('w') as f:
    p=subprocess.Popen(cmd,cwd=S,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait(timeout=7200)
result=dict(private_test_commit='8e087ff43e',parent_private_commit='3843fcb54a',command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,log_sha256=sha(E/'test.log'),source_inputs_sha256=sha(E/'source-inputs.json'),scope='Real Linux mincore subject: idle pages discarded, recent/in-use bytes intact, mapping and bitmap retained, subsequent reuse writable. Separate test checkout; app build/products unchanged.')
(E/'result.json').write_text(json.dumps(result,indent=2)+'\n');assert rc==0
assert 'test result: ok. 1 passed; 0 failed;' in (E/'test.log').read_text()
(E/'complete.exit').write_text('0\n');print(json.dumps(result))
