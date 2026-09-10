from pathlib import Path
import json,subprocess,sys
w=Path(__file__).resolve().parent;root=w.parents[3]
phases=[('validate.py',[]),('build-options.py',['--candidate']),('validate-options.py',['--candidate']),('validate-callback.py',['--candidate']),('validate-existing.py',[]),('validate-inherited-gaps.py',[])]
if '--ir' in sys.argv:phases=[('check-ir.py',[]),('check-extra-ir.py',['--candidate']),('compare-roots.py',[])]
results=[]
for name,args in phases:
 print('START',name,args,flush=True)
 with (w/('run-'+name+'.log')).open('wb') as log:r=subprocess.run(['python3',str(w/name)]+args,cwd=root,stdout=log,stderr=subprocess.STDOUT)
 print('FINISHED',name,r.returncode,flush=True);results.append({'script':name,'arguments':args,'exit_code':r.returncode})
 if r.returncode and name!='check-ir.py':raise SystemExit(r.returncode)
(w/('ir-pipeline.json' if '--ir' in sys.argv else 'validation-pipeline.json')).write_text(json.dumps(results,indent=2)+'\n')
