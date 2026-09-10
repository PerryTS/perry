from pathlib import Path
import json,shlex,subprocess
w=Path(__file__).resolve().parent;prefix='.work/'+w.name+'/';remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';host='perry@perry-macos.local';common=['--node','/opt/homebrew/bin/node','--bun','/Users/perry/.bun/bin/bun']
access=['python3',prefix+'run-access.py','--worker',prefix+'candidate-access-worker','--baseline-worker',prefix+'main-access-worker']+common
focus=['python3',prefix+'run-focus.py','--worker',prefix+'candidate-worker','--baseline-worker',prefix+'main-worker']+common
for case in ['records_array_16k:scan:4000:8:7','records_array_1m:scan:200:2:7','records_array_8m:scan:32:1:7','records_array_20m:scan:16:1:7','records_array_20m:roundtrip:8:1:7','records_array_1m:parse:200:8:7','records_array_1m:stringify:256:8:7','small_record:parse:2000000:5000:7','small_record:stringify:10000000:5000:7','long_string_1m:stringify:4096:8:7','null:stringify:20000000:5000:7','string_a:stringify:20000000:5000:7','empty_object:stringify:10000000:5000:7','tiny_object:stringify:10000000:5000:7','object_1k:stringify:2000000:5000:7']:focus+=['--case',case]
options=['python3',prefix+'run-options.py','--worker',prefix+'candidate-options','--baseline-worker',prefix+'main-options']+common
for kind,cmd in [('access',access),('focus',focus),('options',options)]:
 slug='quiet-'+w.name+'-'+kind;invocation=['python3','with_lock.py','--']+cmd+['--results-dir','results/'+slug]
 (w/('remote-'+kind+'-command.json')).write_text(json.dumps(invocation,indent=2)+'\n')
 with (w/('remote-'+kind+'.log')).open('wb') as log:r=subprocess.run(['ssh',host,'cd '+shlex.quote(remote)+' && '+shlex.join(invocation)],stdout=log,stderr=subprocess.STDOUT)
 print((w/('remote-'+kind+'.log')).read_text(),flush=True)
 if r.returncode:raise SystemExit(r.returncode)
 subprocess.run(['python3',str(w/'archive-results.py'),slug,kind],check=True)
