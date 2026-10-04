"""Private isolated two-file idle-Eden probe. Control remains unchanged."""
import pathlib,json,hashlib,subprocess,os,tarfile,time,shutil
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-old-hole-v34';R.mkdir(exist_ok=False)
S=R/'source';S.mkdir();T=R/'target';E=R/'export';E.mkdir()
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,indent=2)+'\n')
control=json.loads((B/'primary-latest2026-v31/export/gc-source-inputs.json').read_text())
for n in ['perry-latest2026-v31-source.tar.gz','gc-runtime-latest2026-v31-overlay.tar.gz']:
    with tarfile.open(B/n) as t:t.extractall(S,filter='data')
assert all(sha(S/n)==h for n,h in control.items())
provenance=json.loads((B/'gc-old-hole-latest2026-v34-provenance.json').read_text())
assert sha(B/'gc-old-hole-latest2026-v34-overlay.tar.gz')==provenance['overlay_sha256']
with tarfile.open(B/'gc-old-hole-latest2026-v34-overlay.tar.gz') as t:t.extractall(S,filter='data')
overlays=provenance['paths']
inputs={}
for n in sorted(control):p=S/n;p.touch();inputs[n]=sha(p)
assert len(inputs)==4999 and [n for n in inputs if inputs[n]!=control[n]]==sorted(overlays)
save('source-inputs.json',inputs)
save('provenance.json',dict(provenance,source=str(S),target=str(T),script_sha256=sha(__file__)))
subprocess.run(['cp','-a','--reflink=auto',B/'primary-latest2026-v31/target-gc',T],check=True)
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'4','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1','CARGO_TARGET_DIR':str(T),'PERRY_RUNTIME_DIR':str(T/'release'),'PERRY_WORKSPACE_ROOT':str(S)}
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
