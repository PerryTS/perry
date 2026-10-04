"""Private tag-word full-runtime app build, current source bound, no auto knobs."""
import hashlib,json,os,pathlib,subprocess,time
R=pathlib.Path('/root/rss-gc-build-20261003'); E=R/'export-current0ed-tag-v18r1';Q=R/'export-current0ed-v14';S=R/'source-current0ed-tag-v18r1';A=E/'current0ed-tag';(E/'bin').mkdir(exist_ok=True)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
assert (E/'build.exit').read_text().strip()=='0';proof=json.loads((E/'source-and-products.json').read_text());assert all(sha(A/n)==h for n,h in proof['products'].items())
for file,h in proof['source_inputs'].items():assert sha(S/file)==h,file
for stem in ['sources','extra-sources']: (E/stem).symlink_to(Q/stem)
oracle=json.loads((Q/'gc-qb6-current0ed-v14-noauto-oracle.json').read_text());(E/'tag-v18r1-noauto-oracle.json').write_text(json.dumps(oracle,indent=2)+'\n');records=[]
for case in ['tscwork','zodwork']:
 src=Q/'sources/real'/(case+'.ts');binary=E/'bin'/('gc-tag-v18r1-noauto-current0ed-tag-'+case)
 env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'PERRY_RUNTIME_DIR':str(A),'PERRY_WORKSPACE_ROOT':str(S),'PERRY_NO_CACHE':'1','PERRY_KEEP_SYMBOLS':'1','LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'8','CARGO_TARGET_DIR':str(R/'target-current0ed-tag-v18r1'),'RAYON_NUM_THREADS':'8','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0'}
 cmd=[A/'perry','compile',src,'--no-auto-optimize','-o',binary];start=time.time()
 with (E/(case+'-compile.out')).open('w') as out,(E/(case+'-compile.err')).open('w') as err:r=subprocess.run(list(map(str,cmd)),env=env,cwd=E,stdout=out,stderr=err,timeout=1800)
 record=dict(cmd=list(map(str,cmd)),case=case,arm='current0ed-tag',source_sha256=sha(src),compiler_sha256=sha(A/'perry'),rc=r.returncode,reason=None,elapsed_s=time.time()-start);records.append(record);(E/'tag-v18r1-noauto-builds.json').write_text(json.dumps(records,indent=2)+'\n');assert r.returncode==0
 record['binary_sha256']=sha(binary)
 (E/'tag-v18r1-noauto-builds.json').write_text(json.dumps(records,indent=2)+'\n');print('Private tag build complete',case,flush=True)
assert all(sha(A/n)==h for n,h in proof['products'].items());(E/'real-build.exit').write_text('0\n')
