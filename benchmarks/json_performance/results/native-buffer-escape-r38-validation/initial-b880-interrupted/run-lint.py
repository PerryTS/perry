from pathlib import Path
import subprocess,os,json
w=Path(__file__).resolve().parent;root=w.parents[3]
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
records=[]
for name,cmd,extra in [('script-lint',['./scripts/run_lint_gates.sh'],{'SKIP_COMPILE_GATES':'1'}),('file-cap',['./scripts/check_file_size.sh'],{})]:
 with (w/(name+'.log')).open('wb') as log:r=subprocess.run(cmd,cwd=root,env=clean|extra,stdout=log,stderr=subprocess.STDOUT)
 records.append(dict(name=name,source_commit=head,command=cmd,env=extra,exit_code=r.returncode))
 print(name,'terminal',r.returncode,flush=True)
(w/'lint-results.json').write_text(json.dumps(records,indent=2)+'\n')
