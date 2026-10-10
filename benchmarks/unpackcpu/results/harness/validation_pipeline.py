from pathlib import Path
import subprocess,os,time,json,shutil
L=Path('/root/lanes/perry-unpackcpu');R=L/'perry'
while not (L/'compile-fix.rc').exists():time.sleep(15)
assert (L/'compile-fix.rc').read_text().strip()=='0'
while not (L/'profile-final-node.rc').exists():time.sleep(15)
e=os.environ.copy();e.pop('PERRY_NO_AUTO_OPTIMIZE',None);e.pop('PERRY_SKIP_BUILD',None);e.update(PATH=str(L/'build-tools')+':/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:'+e['PATH'],PERRY_LANE_AUTO_TARGET=str(L/'auto-target-validation'),CARGO_TARGET_DIR=str(L/'target'),PERRY_RUNTIME_DIR=str(L/'target/release'),PERRY_WORKSPACE_ROOT=str(R),CARGO_BUILD_JOBS='8',RUST_TEST_THREADS='1',PERRY_KEEP_SYMBOLS='1')
O=L/'validation';O.mkdir(exist_ok=True)
def run(tag,cmd,cwd=R,env=e):
 with (O/(tag+'.log')).open('w') as f:r=subprocess.run(['taskset','-c','0-55',*cmd],cwd=cwd,env=env,stdout=f,stderr=f)
 (O/(tag+'.rc')).write_text(str(r.returncode));print(tag,r.returncode,flush=True);return r.returncode
for arm in ['base','fix']:
 for i in range(5):
  rc=run(f'budget-{arm}-{i}',['python3',str(L/'check_hash_budget.py'),arm,str(i)],L)
  assert rc==(1 if arm=='base' else 0)
masked=e.copy();masked['OPENSSL_ia32cap']=':~0x128'
rc=run('budget-masked-fix',['python3',str(L/'check_hash_budget.py'),'fix','masked'],L,masked);assert rc==1
for arm in ['sha','fix']:
 for i in range(5):
  rc=run(f'inflate-budget-{arm}-{i}',['python3',str(L/'check_inflate_budget.py'),arm,str(i)],L)
  assert rc==(1 if arm=='sha' else 0)
skips=[]
for line in (L/'stdlib-base-tests.log').read_text().splitlines():
 if line.startswith('    runtime_thread_exit_tests::'):skips.append(line.strip())
assert len(skips)==2
args=sum((['--skip',s] for s in skips),[])
assert run('stdlib-filtered',['cargo','test','--release','-p','perry-stdlib','--',*args])==0
for name in ['test_gap_crypto_wide','test_gap_crypto_byte_update_encoding','test_gap_crypto_hash_chain_11516','test_gap_zlib_cpu_inflate','test_gap_native_payload_zlib_one_shots','test_gap_zlib_chunk_backpressure']:
 assert run(name,['./run_parity_tests.sh','--filter',name])==0
 for auto in (R/'target').glob('perry-auto-*'):shutil.rmtree(auto)
shutil.rmtree(L/'auto-target-validation',ignore_errors=True)
for name,cmd in [('node-version',['python3','scripts/check_node_version_consistency.py','--list']),('file-size',['bash','scripts/check_file_size.sh']),('diff',['git','diff','--check'])]:assert run(name,cmd)==0
print('COMPLETE',flush=True)
