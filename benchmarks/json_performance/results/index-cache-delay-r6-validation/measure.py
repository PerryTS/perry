from pathlib import Path
import gzip, hashlib, json, shlex, subprocess
work=Path(__file__).resolve().parent
bench=work.parents[1]
remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
host='perry@perry-macos.local'
candidate=work.name
common=['--node','/opt/homebrew/bin/node','--bun','/Users/perry/.bun/bin/bun']
access=['python3','.work/'+candidate+'/run-access.py','--worker','.work/'+candidate+'/access-worker','--baseline-worker','.work/main-53df-fresh/main-access','--prior-worker','.work/invariant-field-loop-r5/access-worker']+common
focus=['python3','run_regression_focus.py','--worker','.work/'+candidate+'/worker','--baseline-worker','.work/main-53df-fresh/worker','--prior-worker','.work/invariant-field-loop-r5/worker']+common
for case in ['records_array_16k:scan:4000:8:7','records_array_1m:scan:200:2:7','records_array_8m:scan:32:1:7','records_array_20m:scan:16:1:7','records_array_20m:roundtrip:8:1:7','records_array_1m:parse:200:8:7','records_array_1m:stringify:256:8:7','small_record:parse:2000000:5000:7','small_record:stringify:10000000:5000:7','long_string_1m:stringify:4096:8:7']:
 focus+=['--case',case]
for kind,command in [('access',access),('focus',focus)]:
 slug='quiet-'+candidate+'-'+kind
 command+=['--results-dir','results/'+slug]
 invocation=['python3','with_lock.py','--']+command
 (work/('remote-'+kind+'-command.json')).write_text(json.dumps(invocation,indent=2)+'\n')
 with (work/('remote-'+kind+'.log')).open('wb') as log:
  result=subprocess.run(['ssh',host,'cd '+shlex.quote(remote)+' && '+shlex.join(invocation)],stdout=log,stderr=subprocess.STDOUT)
 print((work/('remote-'+kind+'.log')).read_text(),flush=True)
 if result.returncode: raise SystemExit(result.returncode)
 # Archive and qualify the terminal window before any subsequent remote job.
 subprocess.run(['python3',str(work/'archive-results.py'),slug,candidate,kind],check=True)
 dest=bench/'results'/slug
 patch=dest/'source.patch';raw=patch.read_bytes();data=gzip.compress(raw,compresslevel=9,mtime=0)
 assert gzip.decompress(data)==raw
 patch.with_suffix('.patch.gz').write_bytes(data)
 patch.with_suffix('.patch.json').write_text(json.dumps({'original_bytes':len(raw),'original_sha256':hashlib.sha256(raw).hexdigest(),'gzip_sha256':hashlib.sha256(data).hexdigest()},indent=2)+'\n')
 patch.unlink()
 print('ARCHIVED',slug,flush=True)
