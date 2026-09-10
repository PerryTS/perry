from pathlib import Path
import json,shlex,shutil,subprocess
w=Path(__file__).resolve().parent;prefix='.work/'+w.name+'/';remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance';slug='quiet-'+w.name+'-modes'
cmd=['python3','with_lock.py','--','python3',prefix+'run-modes.py','--worker',prefix+'main-worker','--node','/opt/homebrew/bin/node','--bun','/Users/perry/.bun/bin/bun','--results-dir','results/'+slug]
(w/'remote-modes-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
with (w/'remote-modes.log').open('wb') as log:r=subprocess.run(['ssh','perry@perry-macos.local','cd '+shlex.quote(remote)+' && '+shlex.join(cmd)],stdout=log,stderr=subprocess.STDOUT)
print((w/'remote-modes.log').read_text(),flush=True)
# The first subsequent remote operation archives the terminal window even on failure.
subprocess.run(['python3',str(w/'archive-window.py'),slug],check=True)
d=w.parents[1]/'results'/slug
for n in ['remote-modes-command.json','remote-modes.log','remote-stage-hashes.json']:shutil.copy2(w/n,d/n)
(d/'controller-exit.json').write_text(json.dumps({'exit_code':r.returncode,'diagnostic_only':True},indent=2)+'\n')
if r.returncode:raise SystemExit(r.returncode)
