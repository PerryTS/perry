"""Private compiler-layout measurement; no production/runtime ABI changes."""
import pathlib,json,hashlib,subprocess,shutil,os,struct,time
B=pathlib.Path('/root/rss-header-20261002'); R=B/'primary-arena-geometry-v27'; R.mkdir(exist_ok=False)
P=B/'primary-gc-runtime-v24'; S=R/'source'; T=R/'target'; E=R/'export'; E.mkdir()
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def save(n,x):(E/n).write_text(json.dumps(x,indent=2)+'\n')
control=json.loads((P/'export/control-source-inputs.json').read_text())
assert all(sha(P/'source-control'/n)==h for n,h in control.items())
for source,dest in [(P/'source-control',S),(P/'target-control',T)]:subprocess.run(['cp','-a','--reflink=auto',source,dest],check=True)
overlays={
 'crates/perry-runtime/src/arena/block.rs':('private-arena-geometry-v27-block.rs','13612812d6f1b03b045fcc37dbb0ae3489be18c647bd3795e313ac07319cdcf3'),
 'crates/perry-runtime/src/arena/block/rss_observer_geometry.rs':('private-arena-geometry-v27-probe.rs','e27e49ba809376360562aac3099c6a893f28c083ecebb9503e3be5fb16d9a63c')}
for n,(p,h) in overlays.items():assert sha(B/p)==h;shutil.copy2(B/p,S/n)
inputs={}
for p in S.rglob('*'):
 if p.is_file() and (p.suffix=='.rs' or p.name in ['Cargo.toml','Cargo.lock']):p.touch();inputs[str(p.relative_to(S))]=sha(p)
assert [n for n in sorted(set(control)|set(inputs)) if control.get(n)!=inputs.get(n)]==sorted(overlays)
save('source-inputs.json',inputs)
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'8','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1','CARGO_TARGET_DIR':str(T),'PERRY_RUNTIME_DIR':str(T/'release'),'PERRY_WORKSPACE_ROOT':str(S),'RSS_OBSERVER_GEOMETRY_ROOT':str(E/'raw')}
records=[]
for name,cmd in [
 ('build',['cargo','build','--locked','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static']),
 ('geometry',['cargo','test','--locked','--release','-p','perry-runtime','--lib','rss_observer_layout_geometry','--','--nocapture'])]:
 start=time.monotonic()
 with (E/(name+'.log')).open('w') as f:
  proc=subprocess.Popen(cmd,cwd=S,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=proc.wait(timeout=7200)
 records.append(dict(name=name,command=cmd,rc=rc,elapsed_s=time.monotonic()-start,pid=proc.pid,log_sha256=sha(E/(name+'.log'))));save('commands.json',records);assert rc==0
 assert 'Compiling perry-runtime ' in (E/(name+'.log')).read_text()
 print(name,'complete',flush=True)
g=json.loads((E/'raw/geometry.json').read_text());assert g['target_arch']=='x86_64' and g['target_os']=='linux' and g['usize_bytes']==8
for key,filename,fields in [('vec','vec.bin',['pointer','capacity','length']),('box_slice','box.bin',['pointer','length'])]:
 raw=(E/'raw'/filename).read_bytes();assert len(raw)==g[key]['size']==8*len(fields)
 words=struct.unpack('<'+'Q'*len(fields),raw);assert set(words)=={g[key][k] for k in fields};assert len(set(words))==len(fields)
 g[key]['word_offsets']={k:words.index(g[key][k])*8 for k in fields}
assert all(sha(S/n)==h for n,h in inputs.items())
save('geometry-verified.json',dict(geometry=g,private_commit='2571f4839e',control_source_proof_sha256=sha(P/'export/independent-build-verification.json'),inputs_sha256=sha(E/'source-inputs.json'),raw_sha256={n:sha(E/'raw'/n) for n in ['geometry.json','vec.bin','box.bin']},rustc=subprocess.check_output(['rustc','-vV'],env=env,text=True),script_sha256=sha(__file__),scope='Exact test-compiled Linux arena field offsets and opaque std representations for an external observer; no production field changed. Before use, verify shipping access offsets and source geometry.'))
(E/'complete.exit').write_text('0\n')
