from pathlib import Path
import json,os,subprocess,shutil
L=Path('/root/lanes/perry-unpackcpu');R=L/'perry';V=L/'validation'
n=(R/'test-parity/output/node/test_gap_native_payload_zlib_one_shots.txt').read_text().splitlines()
p=(R/'test-parity/output/perry/test_gap_native_payload_zlib_one_shots.txt').read_text().splitlines()
base=dict(line.split() for line in (L/'evidence/one-shot-control.out').read_text().splitlines())
assert len(n)==len(p)==9 and n[3:]==p[3:]
for nl,pl in zip(n[:3],p[:3]):
 ns,ps=nl.split(),pl.split();assert ns[:4]==ps[:4] and ns[1:4]==['true']*3 and ps[4]==ps[5]==base[ps[0]]
record=dict(baseline='8ba3b4fa876b1e0dc03f6de84611108577910d37',producer='unchanged flate2 1.1.10/miniz_oxide 0.9.1 encoders',baseline_native_crc=base,node=n,combined=p,round_trip_and_remaining_output_parity=True)
(L/'evidence/inherited-compression-gap.json').write_text(json.dumps(record,indent=2)+'\n')
e=os.environ.copy();e.pop('PERRY_NO_AUTO_OPTIMIZE',None);e.pop('PERRY_SKIP_BUILD',None)
e.update(PATH=str(L/'build-tools')+':/opt/node-v26.5.1-linux-x64/bin:/root/.cargo/bin:'+e['PATH'],PERRY_LANE_AUTO_TARGET=str(L/'auto-target-validation'),CARGO_TARGET_DIR=str(L/'target'),PERRY_RUNTIME_DIR=str(L/'target/release'),PERRY_WORKSPACE_ROOT=str(R),CARGO_BUILD_JOBS='8',RUST_TEST_THREADS='1',PERRY_KEEP_SYMBOLS='1')
def run(name,cmd):
 with (V/(name+'.log')).open('w') as f:rc=subprocess.run(['taskset','-c','0-55',*cmd],cwd=R,env=e,stdout=f,stderr=f).returncode
 (V/(name+'.rc')).write_text(str(rc));print(name,rc,flush=True);assert rc==0
run('test_gap_zlib_chunk_backpressure',['./run_parity_tests.sh','--filter','test_gap_zlib_chunk_backpressure'])
for auto in (R/'target').glob('perry-auto-*'):shutil.rmtree(auto)
shutil.rmtree(L/'auto-target-validation',ignore_errors=True)
for name,cmd in [('node-version',['python3','scripts/check_node_version_consistency.py','--list']),('file-size',['bash','scripts/check_file_size.sh']),('diff',['git','diff','--check'])]:run(name,cmd)
with (L/'validation_pipeline.log').open('a') as f:f.write('COMPLETE: remaining checks pass; inherited compression CRC gap separately proven.\n')
print('COMPLETE',flush=True)
