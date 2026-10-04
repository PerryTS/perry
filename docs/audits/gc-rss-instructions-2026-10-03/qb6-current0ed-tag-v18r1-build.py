"""Own-source/target/runtime private tag-word experiment; never changes v14.
Refresh all workspace inputs after reusing ONLY our completed runtime cache.
"""
import hashlib,json,os,pathlib,shutil,subprocess,time
R=pathlib.Path('/root/rss-gc-build-20261003');S=R/'source-current0ed-tag-v18r1';T=R/'target-current0ed-tag-v18r1';E=R/'export-current0ed-tag-v18r1';E.mkdir(exist_ok=True)
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
assert sha(R/'current0ed-private-tag-v18-source.tar.gz')=='6e3d949337ddb34f1d956220dbb8f3ddf3be8ef2c00a4cdb32e35dd4a8cc368d';assert not S.exists() and not T.exists();S.mkdir();subprocess.run(['tar','-xzf',R/'current0ed-private-tag-v18-source.tar.gz','-C',S],check=True)
subprocess.run(['cp','-a','--reflink=auto',R/'target-current0ed-gc',T],check=True)
for p in S.rglob('*'):
 if p.is_file() and (p.suffix=='.rs' or p.name in ['Cargo.toml','Cargo.lock']):p.touch()
env=os.environ|{'PATH':'/root/.cargo/bin:'+os.environ['PATH'],'LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'8','CARGO_TARGET_DIR':str(T),'CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0','RUST_TEST_THREADS':'1'}
records=[]
def run(name,cmd,timeout=1800):
 start=time.time()
 with (E/(name+'.out')).open('w') as out,(E/(name+'.err')).open('w') as err:r=subprocess.run(list(map(str,cmd)),cwd=S,env=env,stdout=out,stderr=err,timeout=timeout)
 d=dict(command=list(map(str,cmd)),rc=r.returncode,seconds=time.time()-start,stdout_sha256=sha(E/(name+'.out')),stderr_sha256=sha(E/(name+'.err')));records.append(d);(E/'build-runs.json').write_text(json.dumps(records,indent=2)+'\n');assert r.returncode==0,d
run('compile',['cargo','build','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static'])
run('runtime-tests',['cargo','test','--release','-p','perry-runtime','--lib','--','--test-threads=1'])
A=E/'current0ed-tag';A.mkdir();products={}
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 p=T/'release'/name;shutil.copy2(p,A/name);products[name]=sha(A/name)
inputs={str(p.relative_to(S)):sha(p) for p in S.rglob('*') if p.is_file() and (p.suffix=='.rs' or p.name in ['Cargo.toml','Cargo.lock'])}
(E/'source-and-products.json').write_text(json.dumps(dict(private_commit='e42f30cc60',production_parent='aa6eeebed01e34486e0db25cad1694ded151c978',source_archive_sha256=sha(R/'current0ed-private-tag-v18-source.tar.gz'),source_inputs=inputs,products=products,environment={k:env[k] for k in ['LLVM_SYS_221_PREFIX','CARGO_BUILD_JOBS','CARGO_TARGET_DIR','CARGO_PROFILE_RELEASE_CODEGEN_UNITS','CARGO_INCREMENTAL','RUST_TEST_THREADS']},scope='Private tag-word-only experiment, not applied to PR production source.'),indent=2)+'\n')
(E/'build.exit').write_text('0\n');print('Private tag v18 build and Linux runtime tests complete.',flush=True)
