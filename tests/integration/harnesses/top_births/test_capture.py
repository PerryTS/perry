#!/usr/bin/env python3
"""Functional capture/suppression/off-state check; deliberately fails on base."""
import argparse,json,os,subprocess,tempfile
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--perry',required=True);p.add_argument('--runtime',required=True);p.add_argument('--workspace',required=True);a=p.parse_args()
with tempfile.TemporaryDirectory(prefix='birth-capture-') as td:
 r=Path(td);(r/'input.ts').write_text('const source: any = JSON.parse(\'{"value":7}\');\nfunction pass(v:any):any{return v;}\nconsole.log(pass(source).value);\n')
 env=dict(os.environ,PERRY_RUNTIME_DIR=a.runtime,PERRY_WORKSPACE_ROOT=a.workspace,PERRY_FACTS_ONLY='1',PERRY_FACTS_UNITS='0',PERRY_ALLOW_PERRY_FEATURES='1',PERRY_FACTS_BIRTH_CENSUS='1')
 env.pop('PERRY_FACTS_EXPLAIN',None);env.pop('PERRY_FACTS_SUPPRESS_BIRTH',None)
 def run(name):
  env['PERRY_FACTS_DUMP']=str(r/(name+'.dump'))
  with (r/(name+'.log')).open('w') as log:subprocess.run(['timeout','180',a.perry,'compile','--cache-dir',str(r/('cache-'+name)),str(r/'input.ts'),'-o',str(r/('output-'+name))],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
  return [json.loads(l[2:]) for l in open(r/(name+'.dump')) if l.startswith('D {')]
 records=run('on');births=[x for x in records if x['record']=='birth-injection'];assert births,'compiler emitted no birth records'
 b=next(x for x in births if x['why']=='thrown value (runtime errors)')
 env['PERRY_FACTS_SUPPRESS_BIRTH']=f"{b['injector']}:{b['line']}:{b['node']}"
 arm=run('suppressed');assert sum(x['suppressed'] for x in arm if x['record']=='birth-meta')>0
 assert not any(x['record']=='node' and x['id']==b['node'] and x['top'] for x in arm)
 env.pop('PERRY_FACTS_SUPPRESS_BIRTH');env.pop('PERRY_FACTS_BIRTH_CENSUS')
 off=run('off');assert not any(x['record'].startswith('birth-') for x in off)
 print('PASS: birth capture, effective single-origin suppression, census-off absence')
