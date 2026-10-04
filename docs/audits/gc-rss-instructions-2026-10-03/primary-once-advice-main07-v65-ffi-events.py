"""Fresh isolated FFI and event root-scanner tests on the proposed runtime."""
import pathlib,json,hashlib,subprocess,shutil,os,time
B=pathlib.Path('/root/rss-header-20261002');P=B/'primary-entry-once-main07-v45';R=B/'primary-once-advice-main07-v65-ffi-events';R.mkdir(exist_ok=False)
S=R/'source';T=R/'target';E=R/'export';E.mkdir()
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
shutil.copytree(P/'source',S,ignore=shutil.ignore_patterns('target','.perry-cache','.git'))
inputs=json.loads((P/'export/source-inputs.json').read_text());assert len(inputs)==5011 and all(sha(S/n)==h for n,h in inputs.items())
for n in inputs:(S/n).touch()
(E/'source-inputs.json').write_text(json.dumps(inputs,indent=2)+'\n')
subprocess.run(['cp','-a','--reflink=auto',B/'primary-latest2026-v31/target-gc',T],check=True)
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'CARGO_BUILD_JOBS':'4','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1','CARGO_TARGET_DIR':str(T),'PERRY_RUNTIME_DIR':str(T/'release'),'PERRY_WORKSPACE_ROOT':str(S)}
cmd=['cargo','test','--locked','--release','-p','perry-ffi','-p','perry-ext-events']
start=time.monotonic()
with (E/'test.log').open('w') as f:
 p=subprocess.Popen(cmd,cwd=S,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait(timeout=7200)
result=dict(base='07e50b14a3d23e8f914193ec4d5a679adbd7c37a',production_runtime='4401b3b981',source_equivalent_to='8e3013733da37ddc66998ee51b9bd1109a9175ec',command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,log_sha256=sha(E/'test.log'),source_inputs_sha256=sha(E/'source-inputs.json'))
(E/'result.json').write_text(json.dumps(result,indent=2)+'\n');assert rc==0
assert all(sha(S/n)==h for n,h in inputs.items())
(E/'complete.exit').write_text('0\n');print(json.dumps(result))
