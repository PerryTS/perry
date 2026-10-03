"""Three isolated arms for private GC helper probes on the original host.
Only owned qb6 cache is seeded; every workspace Rust/Cargo input is refreshed.
The control is the current production GC proposal, not pristine main.
"""
import pathlib,json,hashlib,subprocess,os,tarfile,time,shutil,concurrent.futures
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-gc-runtime-v24';R.mkdir(exist_ok=False);E=R/'export';E.mkdir();(E/'logs').mkdir()
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,indent=2)+'\n')
p=json.loads((B/'provenance-gc-runtime-currentc170-v22.json').read_text());archives={n:sha(B/n) for n in ['perry-currentc170-source.tar.gz','gc-runtime-currentc170-v22-source.tar.gz']}
assert archives=={'perry-currentc170-source.tar.gz':'a65f054acb9226b65c30eb70576a2fadb9acf9ec40226dd13b0ecb4eaa424052','gc-runtime-currentc170-v22-source.tar.gz':'2934d8f3119246ec23dc707a4672c663bf599e1602a206b7d0b1ece0c3fbc57a'}
overlays={'weak':('crates/perry-runtime/src/gc/verify.rs',B/'private-weak-v24-verify.rs'),'elf':('crates/perry-runtime/src/gc/roots/runtime_handles.rs',B/'private-elf-roots-v25-runtime-handles.rs')}
assert {a:sha(v[1]) for a,v in overlays.items()}=={'weak': '170098f50c84a3bf9a7c069700dd93ccc7248e21ecc84d120e8d6e8515d3937d', 'elf': 'af1c4c58d93237818a7a19ce8455466a9a5f8f8dcd56157180ff26ba24dfb39b'}
for arm in ['control','weak','elf']:
 s=R/('source-'+arm);s.mkdir();t=R/('target-'+arm)
 for n in archives:
  with tarfile.open(B/n) as a:a.extractall(s,filter='data')
 assert all(sha(s/n)==h for n,h in p['files'].items())
 if arm in overlays:
  n,path=overlays[arm];shutil.copy2(path,s/n)
 subprocess.run(['cp','-a','--reflink=auto',B/'primary-weak-v24-owned-cache',t],check=True)
 inputs={}
 for f in s.rglob('*'):
  if f.is_file() and (f.suffix=='.rs' or f.name in ['Cargo.toml','Cargo.lock']):f.touch();inputs[str(f.relative_to(s))]=sha(f)
 save(arm+'-source-inputs.json',inputs)
save('provenance.json',dict(base=p['base'],production_source=p['head'],control='current production GC proposal',private_commits={'weak':'ff68bb0578','elf':'a609076923'},archives=archives,overlays={a:{'path':v[0],'sha256':sha(v[1])} for a,v in overlays.items()},host=subprocess.check_output(['hostname'],text=True).strip(),script_sha256=sha(__file__)))
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'8','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1'}
def build(arm):
 s=R/('source-'+arm);t=R/('target-'+arm);ae=env|{'CARGO_TARGET_DIR':str(t),'PERRY_RUNTIME_DIR':str(t/'release'),'PERRY_WORKSPACE_ROOT':str(s)};records=[]
 def run(n,cmd,timeout):
  start=time.monotonic()
  with (E/'logs'/(arm+'-'+n+'.log')).open('w') as out:
   proc=subprocess.Popen(cmd,cwd=s,env=ae,stdout=out,stderr=subprocess.STDOUT,start_new_session=True)
   rc=proc.wait(timeout=timeout)
  row=dict(name=n,command=cmd,rc=rc,elapsed_seconds=time.monotonic()-start,pid=proc.pid,log_sha256=sha(E/'logs'/(arm+'-'+n+'.log')),source=str(s),target=str(t));records.append(row);save(arm+'-commands.json',records);assert rc==0,row
 run('build',['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static'],7200)
 product=E/arm;product.mkdir()
 for n in ['perry','libperry_runtime.a','libperry_stdlib.a']:shutil.copy2(t/'release'/n,product/n)
 save(arm+'-products.json',{n.name:sha(n) for n in product.iterdir()});(E/(arm+'-build.exit')).write_text('0\n');print(arm,'products exported',flush=True)
 if arm!='control':run('runtime-tests',['cargo','test','--locked','--release','-p','perry-runtime','--lib'],5400)
 (E/(arm+'-complete.exit')).write_text('0\n');return arm
success=False
try:
 with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
  for a in pool.map(build,['control','weak','elf']):print(a,'complete',flush=True)
 success=True
finally:(E/'build-driver.exit').write_text('0\n' if success else '1\n')
