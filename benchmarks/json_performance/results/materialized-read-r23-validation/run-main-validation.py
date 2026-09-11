from pathlib import Path
import subprocess
w=Path(__file__).resolve().parent
for step in ['build-workers.py','validate-fixtures.py','validate-options.py','probe-lazy-baseline.py','check-roots.py']:
 with (w/('main-step-'+step+'.log')).open('wb') as log:
  r=subprocess.run(['python3',str(w/step),'--main'],stdout=log,stderr=subprocess.STDOUT)
 print(step,r.returncode,flush=True)
 assert r.returncode==0,step
