"""Private isolated two-file idle-Eden probe. Control remains unchanged."""
import pathlib,json,hashlib,subprocess,os,tarfile,time,shutil
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-idle-eden-v30';R.mkdir(exist_ok=False)
S=R/'source';S.mkdir();T=R/'target';E=R/'export';E.mkdir()
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,indent=2)+'\n')
control=json.loads((B/'primary-gc-runtime-v24/export/control-source-inputs.json').read_text())
for n in ['perry-currentc170-source.tar.gz','gc-runtime-currentc170-v22-source.tar.gz']:
    with tarfile.open(B/n) as t:t.extractall(S,filter='data')
assert all(sha(S/n)==h for n,h in control.items())
overlays={'crates/perry-runtime/src/arena/block.rs':B/'private-idle-eden-v30-block.rs','crates/perry-runtime/src/arena/reset.rs':B/'private-idle-eden-v30-reset.rs'}
for n,p in overlays.items():shutil.copy2(p,S/n)
inputs={}
for n in sorted(control):p=S/n;p.touch();inputs[n]=sha(p)
assert len(inputs)==4994 and [n for n in inputs if inputs[n]!=control[n]]==sorted(overlays)
save('source-inputs.json',inputs)
save('provenance.json',dict(private_commit='3843fcb54a',base='c17090892e2a749bb4a3f2a0022e0f8274f0320c',production_runtime='f60e41c5ba519167252e94248dea8e9090990eea',overlays={n:sha(p) for n,p in overlays.items()},source=str(S),target=str(T),script_sha256=sha(__file__)))
subprocess.run(['cp','-a','--reflink=auto',B/'primary-weak-v24-owned-cache',T],check=True)
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'8','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1','CARGO_TARGET_DIR':str(T),'PERRY_RUNTIME_DIR':str(T/'release'),'PERRY_WORKSPACE_ROOT':str(S)}
records=[]
for name,cmd in [('build',['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static']),('runtime-tests',['cargo','test','--locked','--release','-p','perry-runtime','--lib'])]:
    start=time.monotonic()
    with (E/(name+'.log')).open('w') as out:
        p=subprocess.Popen(cmd,cwd=S,env=env,stdout=out,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait(timeout=7200)
    records.append(dict(name=name,command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,log_sha256=sha(E/(name+'.log'))));save('commands.json',records)
    assert rc==0 and 'Compiling perry-runtime ' in (E/(name+'.log')).read_text(),records[-1]
    if name=='build':
        for n in ['perry','libperry_runtime.a','libperry_stdlib.a']:shutil.copy2(T/'release'/n,E/n)
        save('products.json',{n:sha(E/n) for n in ['perry','libperry_runtime.a','libperry_stdlib.a']})
    print(name,'passed',flush=True)
assert all(sha(S/n)==h for n,h in inputs.items())
(E/'complete.exit').write_text('0\n')
