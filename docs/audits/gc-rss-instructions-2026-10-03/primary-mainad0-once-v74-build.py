"""Isolated adopted-once native-map prefix build and full runtime tests."""
import pathlib,json,hashlib,subprocess,os,tarfile,time,shutil
B=pathlib.Path('/root/rss-header-20261002');P=B/'primary-entry-once-main07-v45';R=B/'primary-mainad0-once-v74';R.mkdir(exist_ok=False);E=R/'export';E.mkdir();S=R/'source';T=R/'target'

def build_free_bytes():
    # Root jobs can use reserved blocks; retain the same 12GiB minimum.
    space=os.statvfs(B)
    return (space.f_bfree if os.geteuid()==0 else space.f_bavail)*space.f_frsize

def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,indent=2)+'\n')
assert (P/'export/complete.exit').read_text().strip()=='0'
proof=json.loads((B/'mainad0-once-v74-provenance.json').read_text())
assert sha(B/'mainad0-once-v74-overlay.tar.gz')==proof['overlay_sha256']
base_inputs=json.loads((P/'export/source-inputs.json').read_text())
shutil.copytree(P/'source',S,ignore=shutil.ignore_patterns('target','.perry-cache','.git'))
assert len(base_inputs)==5011 and all(sha(S/n)==h for n,h in base_inputs.items())
with tarfile.open(B/'mainad0-once-v74-overlay.tar.gz') as t:t.extractall(S,filter='data')
for n in proof['deleted_paths']:
 path=S/n
 if path.exists():path.unlink()
inputs=json.loads((B/'mainad0-once-v74-inputs.json').read_text())
assert len(inputs)==5014 and all(sha(S/n)==h for n,h in inputs.items())
assert sorted(n for n in inputs if base_inputs.get(n)!=inputs[n])==sorted(proof['private_paths'])
for n in inputs:(S/n).touch()
save('source-inputs.json',inputs);save('provenance.json',proof|dict(script_sha256=sha(__file__)))
assert build_free_bytes()>=12*2**30,'need12GiB build headroom'
subprocess.run(['cp','-a','--reflink=auto',B/'primary-latest2026-v31/target-gc',T],check=True)
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'4','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1','CARGO_TARGET_DIR':str(T),'PERRY_RUNTIME_DIR':str(T/'release'),'PERRY_WORKSPACE_ROOT':str(S)}
records=[]
for name,cmd in [('build',['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static']),('runtime-tests',['cargo','test','--locked','--release','-p','perry-runtime','--lib'])]:
 start=time.monotonic()
 with (E/(name+'.log')).open('w') as f:
  p=subprocess.Popen(cmd,cwd=S,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait(timeout=7200)
 records.append(dict(name=name,command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,log_sha256=sha(E/(name+'.log'))));save('commands.json',records)
 assert rc==0 and 'Compiling perry-runtime ' in (E/(name+'.log')).read_text(),records[-1]
 if name=='build':
  for n in ['perry','libperry_runtime.a','libperry_stdlib.a']:shutil.copy2(T/'release'/n,E/n)
  save('products.json',{n:sha(E/n) for n in ['perry','libperry_runtime.a','libperry_stdlib.a']})
 print(name,'passed',flush=True)
assert all(sha(S/n)==h for n,h in inputs.items())
(E/'complete.exit').write_text('0\n')
# This owned cache has no downstream consumer: exports/source are the inputs.
shutil.rmtree(T)
