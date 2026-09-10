from pathlib import Path
import datetime,hashlib,json,os,shutil,subprocess
root=Path('/Users/amlug/projects/perry/json-merged-pr10022');os.chdir(root)
expected='a1b39840283ce7c771a776706621a9ca249a8e8e'
work=root/'benchmarks/json_performance/.work/public-baseline-refresh'
work.mkdir(exist_ok=False)
head=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip();assert head==expected
assert not subprocess.check_output(['git','status','--porcelain'],text=True).strip()
assert not (root/'.bench-results/public').exists()
# The canonical compute runner uses this fixed scratch root. Reserve it only
# when absent; never delete or take over another session's directory.
scratch=Path('/tmp/perry_polyglot_bench');scratch.mkdir(exist_ok=False)
(scratch/'OWNER').write_text('Codex JSON PR10036 public baseline refresh at '+head+'\n')
outputs=[]
for name in ['rust','zig','perry','node','bun']:
 p=Path('/tmp/out_'+name+'.json')
 with p.open('xb'):pass
 outputs.append(str(p))
env={k:v for k,v in os.environ.items() if not k.startswith(('PERRY_','CARGO_PROFILE_','PUBLIC_BENCH_','HONEST_BENCH_')) and k not in {'RUSTFLAGS','CARGO_TARGET_DIR','NODE_OPTIONS','BUN_OPTIONS'}}
node=root/'benchmarks/json_performance/.work/public-baseline-toolchain/node-v22.23.1-darwin-arm64/bin'
env['PATH']=str(node)+':'+str(Path.home()/'.bun/bin')+':'+env['PATH']
env['PERRY_RUNTIME_DIR']=str(root/'target/release')
env['PERRY_ZIG_CACHE_DIR']=str(work/'zig-cache')
assert subprocess.check_output(['node','--version'],env=env,text=True).strip()=='v22.23.1'
assert subprocess.check_output(['bun','--version'],env=env,text=True).strip()=='1.3.14'
record={'source_commit':head,'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':['./benchmarks/run_public_baseline.sh'],'pinned_node_bin':str(node),'pinned_bun_bin':str(Path.home()/'.bun/bin/bun'),'runtime_dir':env['PERRY_RUNTIME_DIR'],'scratch':str(scratch),'reserved_output_files':outputs,'host':subprocess.check_output(['uname','-a'],text=True).strip(),'purpose':'Regenerate stale published performance evidence required by lint; separate from the M1 Node26 JSON comparison.'}
(work/'provenance.json').write_text(json.dumps(record,indent=2)+'\n')
(work/'processes-before.txt').write_text(subprocess.check_output(['ps','-axo','pid,ppid,etime,pcpu,command'],text=True))
print('START',head,flush=True)
try:
 result=subprocess.run(record['command'],env=env)
 record['exit_code']=result.returncode
finally:
 record['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
 record['final_commit']=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
 record['source_unchanged']=subprocess.run(['git','diff','--quiet',head,'--','crates','Cargo.toml','Cargo.lock','benchmarks/public-baseline-config.json']).returncode==0
 (work/'processes-after.txt').write_text(subprocess.check_output(['ps','-axo','pid,ppid,etime,pcpu,command'],text=True))
 (work/'provenance.json').write_text(json.dumps(record,indent=2)+'\n')
assert record['final_commit']==head and record['source_unchanged']
raise SystemExit(record['exit_code'])
