"""Fresh main checkpoint baseline, production and private Eden policy; isolated products."""
import pathlib,json,hashlib,subprocess,os,tarfile,time,shutil
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-main07-v39';R.mkdir(exist_ok=False);E=R/'export';E.mkdir()
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,indent=2)+'\n')
proof=json.loads((B/'provenance-main07-v39.json').read_text())
assert sha(B/'main07-v39-upstream.patch')==proof['upstream_patch_sha256']
assert sha(B/'gc-runtime-main07-v39-overlay.tar.gz')==proof['overlay_archive_sha256']
previous=B/'primary-latest2026-v31'
old_inputs=json.loads((previous/'export/base-source-inputs.json').read_text())
private=json.loads((B/'primary-entry-eden-v32/export/source-inputs.json').read_text())
control=json.loads((previous/'export/gc-source-inputs.json').read_text())
private_paths=[n for n in private if private[n]!=control[n]];assert len(private_paths)==6
for arm in ['base','gc','entry']:
 S=R/('source-'+arm);T=R/('target-'+arm);out=E/arm;out.mkdir()
 shutil.copytree(previous/'source-base',S,ignore=shutil.ignore_patterns('target','.perry-cache','.git'))
 assert all(sha(S/n)==h for n,h in old_inputs.items())
 subprocess.run(['git','apply','--check',str(B/'main07-v39-upstream.patch')],cwd=S,check=True)
 subprocess.run(['git','apply',str(B/'main07-v39-upstream.patch')],cwd=S,check=True)
 if arm!='base':
  with tarfile.open(B/'gc-runtime-main07-v39-overlay.tar.gz') as t:t.extractall(S,filter='data')
 if arm=='entry':
  # Campaign upstream does not touch these six exact shipping probe inputs.
  for n in private_paths:
   assert sha(S/n)==control[n],('upstream changed private policy integration path',n)
   shutil.copy2(B/'primary-entry-eden-v32/source'/n,S/n)
 manifest=B/('main07-v39-'+('base' if arm=='base' else 'gc')+'-inputs.json')
 inputs=json.loads(manifest.read_text())
 if arm=='entry':
  for n in private_paths:inputs[n]=private[n]
 assert len(inputs)==proof['counts']['base' if arm=='base' else 'gc'] and all(sha(S/n)==h for n,h in inputs.items())
 for n in inputs:(S/n).touch()
 save(arm+'-source-inputs.json',inputs)
 subprocess.run(['cp','-a','--reflink=auto',previous/'target-gc',T],check=True)
 env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'4','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1','CARGO_TARGET_DIR':str(T),'PERRY_RUNTIME_DIR':str(T/'release'),'PERRY_WORKSPACE_ROOT':str(S)}
 records=[]
 for name,cmd in [('build',['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static']),('runtime-tests',['cargo','test','--locked','--release','-p','perry-runtime','--lib'])]:
  start=time.monotonic()
  with (out/(name+'.log')).open('w') as f:
   p=subprocess.Popen(cmd,cwd=S,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait(timeout=7200)
  records.append(dict(name=name,command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,log_sha256=sha(out/(name+'.log'))));save(arm+'-commands.json',records)
  assert rc==0 and 'Compiling perry-runtime ' in (out/(name+'.log')).read_text(),records[-1]
  if name=='build':
   for n in ['perry','libperry_runtime.a','libperry_stdlib.a']:shutil.copy2(T/'release'/n,out/n)
   save(arm+'-products.json',{n:sha(out/n) for n in ['perry','libperry_runtime.a','libperry_stdlib.a']})
  print(arm,name,'passed',flush=True)
 assert all(sha(S/n)==h for n,h in inputs.items())
 (out/'complete.exit').write_text('0\n')
 # This unique arm target has no other consumers. Keep source, products and raw
 # logs; reclaim reproducible cache now so later arms do not multiply disk use.
 shutil.rmtree(T)
 print(arm,'completed; own reproducible target reclaimed',flush=True)
save('provenance.json',proof|dict(private_entry='6e311851fa6a6e0c981b7e2f2c34d3bdc5d3d2f9',private_paths=private_paths,script_sha256=sha(__file__)))
(E/'complete.exit').write_text('0\n')
