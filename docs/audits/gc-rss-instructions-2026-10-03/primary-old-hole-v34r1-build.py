"""New namespace for Linux test-helper visibility fix; preserve original failure."""
import pathlib,json,hashlib,subprocess,os,time,shutil
B=pathlib.Path('/root/rss-header-20261002');P=B/'primary-old-hole-v34';R=B/'primary-old-hole-v34r1';R.mkdir(exist_ok=False)
S=R/'source';T=R/'target';E=R/'export';E.mkdir()
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,indent=2)+'\n')
assert (B/'gc-old-hole-v34-build-driver.exit').read_text().strip()=='1'
assert 'error[E0603]' in (P/'export/runtime-tests.log').read_text()
shutil.copytree(P/'source',S,ignore=shutil.ignore_patterns('target','.perry-cache','.git'))
inputs=json.loads((P/'export/source-inputs.json').read_text());assert len(inputs)==4999 and all(sha(S/n)==h for n,h in inputs.items())
n='crates/perry-runtime/src/gc/old_free.rs';shutil.copy2(B/'old-hole-v34r1-old-free.rs',S/n);inputs[n]=sha(S/n)
save('source-inputs.json',inputs)
subprocess.run(['cp','-a','--reflink=auto',P/'target',T],check=True)
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
save('provenance.json',dict(base='2026ecfe6dd9df1a0a8616e3bc5c5561e8e0cf63',production_runtime='6c761801fa4c76ad79b67bbc4338bee7739093b7',private_test_fix='f22104289b',original_private='d1a45c55d7',scope='Test-only fresh-thread helper fix; shipping policy remains the same. Original E0603 failure retained.',paths=json.loads((P/'export/provenance.json').read_text())['paths'],script_sha256=sha(__file__)))
(E/'complete.exit').write_text('0\n')
