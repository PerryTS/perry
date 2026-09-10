from pathlib import Path
import json,shlex,shutil,subprocess
w=Path(__file__).resolve().parent;prefix='.work/'+w.name+'/';remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';slug='quiet-'+w.name+'-profiles'
cmd=['python3','with_lock.py','--','python3',prefix+'run-profiles.py','--results-dir','results/'+slug]
(w/'remote-profiles-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with (w/'remote-profiles.log').open('wb') as log:r=subprocess.run(['ssh','perry@perry-macos.local','cd '+shlex.quote(remote)+' && '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT)
print((w/'remote-profiles.log').read_text(),flush=True)
subprocess.run(['python3',str(w/'archive-window.py'),slug],check=True)
d=w.parents[1]/'results'/slug
for n in ['remote-profiles-command.json','remote-profiles.log','remote-profile-stage-hashes.json']:shutil.copy2(w/n,d/n)
(d/'controller-exit.json').write_text(json.dumps({'exit_code':r.returncode,'diagnostic_only':True,'instrumented':True},indent=2)+'\n')
if r.returncode:raise SystemExit(r.returncode)
