from pathlib import Path
import subprocess
w=Path(__file__).resolve().parent;root=w.parents[3]
for step in [['build-workers.py','--main'],['validate-fixtures.py','--main'],['validate-options.py'],['probe-lazy-baseline.py'],['check-roots.py','--main']]:
 print('START',step,flush=True)
 with (w/('main-step-'+step[0]+'.log')).open('wb') as log:r=subprocess.run(['python3',str(w/step[0]),*step[1:]],cwd=root,stdout=log,stderr=subprocess.STDOUT)
 print('DONE',step,r.returncode,flush=True)
 if r.returncode:raise SystemExit(r.returncode)
