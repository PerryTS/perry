"""Real collection residency contract plus a control that must detect its absence."""
import pathlib,json,hashlib,subprocess,shutil,os,time
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-entry-eden-v35r1-collection-test'
assert (B/'gc-entry-eden-v35-collection-test-driver.exit').exists(), 'preserve original fixture outcome first'
R.mkdir(exist_ok=False)
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
records=[]
for arm in ['entry','control']:
 P=B/('primary-entry-eden-v32' if arm=='entry' else 'primary-latest2026-v31')
 original=P/('source' if arm=='entry' else 'source-gc')
 manifest=P/'export'/('source-inputs.json' if arm=='entry' else 'gc-source-inputs.json')
 S=R/('source-'+arm);T=R/('target-'+arm);E=R/('export-'+arm);E.mkdir()
 shutil.copytree(original,S,ignore=shutil.ignore_patterns('target','.perry-cache','.git'))
 # Reuse compilation outputs only in this job's isolated directory.
 prior=B/'primary-entry-eden-v35-collection-test/target' if arm=='entry' else P/'target-gc'
 subprocess.run(['cp','-a','--reflink=auto',prior,T],check=True)
 inputs=json.loads(manifest.read_text());assert len(inputs)==4999 and all(sha(S/n)==h for n,h in inputs.items())
 for n,p in {'crates/perry-runtime/src/gc/tests/mod.rs':B/'entry-eden-v35-test-mod.rs','crates/perry-runtime/src/gc/tests/eden_entry_residency.rs':B/'entry-eden-v35r1-real-collection-test.rs'}.items():
  shutil.copy2(p,S/n);inputs[n]=sha(p)
 assert len(inputs)==5000
 (E/'source-inputs.json').write_text(json.dumps(inputs,indent=2)+'\n')
 env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'CARGO_BUILD_JOBS':'4','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1','CARGO_TARGET_DIR':str(T),'PERRY_RUNTIME_DIR':str(T/'release'),'PERRY_WORKSPACE_ROOT':str(S)}
 cmd=['cargo','test','--locked','--release','-p','perry-runtime','--lib','real_full_collection_gives_eden_a_reuse_interval_then_discards_idle_pages','--','--nocapture']
 start=time.monotonic()
 with (E/'test.log').open('w') as f:
  p=subprocess.Popen(cmd,cwd=S,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True);rc=p.wait(timeout=7200)
 text=(E/'test.log').read_text()
 result=dict(arm=arm,command=cmd,rc=rc,pid=p.pid,elapsed_s=time.monotonic()-start,log_sha256=sha(E/'test.log'),source_inputs_sha256=sha(E/'source-inputs.json'),scope='Test-only fixture overlay, actual strings and full collections; no manually written idle ages. Control omits the private shipping policy.')
 records.append(result);(R/'commands.json').write_text(json.dumps(records,indent=2)+'\n')
 assert all(sha(S/n)==h for n,h in inputs.items())
 if arm=='entry':assert rc==0 and 'test result: ok. 1 passed; 0 failed;' in text,text[-3000:]
 else:
  assert rc==101 and 'test result: FAILED. 0 passed; 1 failed;' in text,text[-3000:]
  assert 'next real collection entry discards unused interior pages' in text, 'control must fail the residency contract, not a fixture precondition'
 print(arm,'expected result verified',rc,flush=True)
(R/'complete.exit').write_text('0\n')
