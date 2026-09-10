from pathlib import Path
import gzip, hashlib, json, shlex, shutil, subprocess
w=Path(__file__).resolve().parent
bench=w.parents[1]
remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
slug='quiet-scalar-projection-r4-tiny-crossover'
command=['python3','with_lock.py','--','python3','run_regression_focus.py',
 '--worker','.work/scalar-projection-r3/worker',
 '--baseline-worker','.work/main-53df-fresh/worker',
 '--prior-worker','.work/scalar-projection-r4/main-codegen-r3-runtime-worker',
 '--node','/opt/homebrew/bin/node','--bun','/Users/perry/.bun/bin/bun',
 '--case','small_record:stringify:10000000:5000:9',
 '--results-dir','results/'+slug]
(w/'tiny-crossover-command.json').write_text(json.dumps(command,indent=2)+'\n')
with (w/'tiny-crossover-remote.log').open('wb') as log:
 result=subprocess.run(['ssh','perry@perry-macos.local','cd '+shlex.quote(remote)+' && '+shlex.join(command)],stdout=log,stderr=subprocess.STDOUT)
print((w/'tiny-crossover-remote.log').read_text(),flush=True)
if result.returncode:raise SystemExit(result.returncode)
# This diagnostic times R3 + the crossover, so archive R3's measured provenance.
subprocess.run(['python3',str(w/'archive-results.py'),slug,'scalar-projection-r3','focus'],check=True)
dest=bench/'results'/slug
host=json.loads((dest/'host.json').read_text())
for engine,p in [('perry',w.parent/'scalar-projection-r3/worker'),('baseline',w.parent/'main-53df-fresh/worker'),('prior',w/'main-codegen-r3-runtime-worker')]:
 assert host['workers'][engine]==hashlib.sha256(p.read_bytes()).hexdigest(),engine
raw=(dest/'source.patch').read_bytes();data=gzip.compress(raw,compresslevel=9,mtime=0);assert gzip.decompress(data)==raw
(dest/'source.patch.gz').write_bytes(data)
(dest/'source.patch.json').write_text(json.dumps({'original_bytes':len(raw),'original_sha256':hashlib.sha256(raw).hexdigest(),'gzip_sha256':hashlib.sha256(data).hexdigest()},indent=2)+'\n')
(dest/'source.patch').unlink()
shutil.copy2(w/'hybrid-provenance.json',dest/'hybrid-provenance.json')
(dest/'diagnostic.json').write_text(json.dumps({'note':'R4 investigation of the R3 tiny-stringify regression. perry=R3 object+R3 runtime; baseline=main object+main runtime; prior=main object+R3 runtime. R4 product binary is not timed here. All three measured worker hashes verified after archiving.','engines':{'perry':'R3','baseline':'main','prior':'main codegen + R3 runtime'}},indent=2)+'\n')
print('ARCHIVED',slug,flush=True)
