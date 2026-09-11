from pathlib import Path
import hashlib,json,os,re,subprocess,sys
w=Path(__file__).resolve().parent;root=w.parents[3]
arm='candidate' if '--candidate' in sys.argv else 'main'
build=w/('frozen-build' if arm=='candidate' else 'frozen-main')
source=root/'test-files/test_json_index_remainder.ts'
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
assert subprocess.check_output(['/opt/homebrew/bin/node','--version'],text=True).strip()=='v26.5.1'
node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source)],env=clean,capture_output=True,timeout=180)
for ext,data in [('stdout',node.stdout),('stderr',node.stderr)]: (w/('remainder-node.'+ext)).write_bytes(data)
assert node.returncode==0
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
record=dict(arm=arm,source_commit=json.loads((w/('build-provenance.json' if arm=='candidate' else 'main-build-provenance.json')).read_text())['source_commit'],files={str(p.relative_to(root)):sha(p) for p in [source,build/'perry',build/'libperry_runtime.a',build/'libperry_stdlib.a']},compiles=[],rows=[])
stress={'PERRY_GC_SCHEDULE_SEED':'10022','PERRY_GC_SCHEDULE_RATE':'0.1','PERRY_GC_SCHEDULE_ALLOC_KB':'0','PERRY_GC_PROTECT_FROMSPACE':'1','PERRY_GC_DIAG':'1'}
for roots,setting in [('native','1'),('shadow','0')]:
 binary=w/(arm+'-remainder-'+roots)
 cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','-o',str(binary)]
 overrides={'PERRY_RUNTIME_DIR':str(build),'PERRY_RS4GC':setting,'PERRY_GC_MOVING_LOOP_POLLS':'1'}
 with (w/(binary.name+'-compile.log')).open('wb') as log:subprocess.run(cmd,cwd=root,env=clean|overrides,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 record['compiles'].append(dict(command=cmd,env_overrides=overrides,binary_sha256=sha(binary)))
 cases=[('auto',{},mode,knobs) for mode,knobs in [('normal',{}),('scheduled',stress),('fullgc',{'PERRY_GEN_GC':'0'})]]
 for parser,settings,mode,knobs in cases:
  label=arm+'-remainder-'+roots+'-'+parser+'-'+mode
  try:
   result=subprocess.run([str(binary)],env=clean|settings|knobs,capture_output=True,timeout=180)
   code,out,err=result.returncode,result.stdout,result.stderr
  except subprocess.TimeoutExpired as ex:code,out,err=124,ex.stdout or b'',ex.stderr or b''
  for ext,data in [('stdout',out),('stderr',err)]: (w/(label+'.'+ext)).write_bytes(data)
  diag=err.decode(errors='replace')
  protected=len(re.findall(r'\[gc-fromspace-protect\].*retired_set=#',diag))
  moved=sum(sum(map(int,re.findall(r'\b(?:copied_objects|promoted_objects)=(\d+)',line))) for line in diag.splitlines() if line.startswith('[gc-copy-minor] ran'))
  row=dict(arm=arm,roots=roots,parser=parser,mode=mode,exit_code=code,matches_node=out==node.stdout,protected_retired_sets=protected,moved_objects=moved,env_overrides=settings|knobs,stdout_sha256=hashlib.sha256(out).hexdigest(),stderr_sha256=hashlib.sha256(err).hexdigest())
  record['rows'].append(row)
  (w/(arm+'-remainder-validation.json')).write_text(json.dumps(record,indent=2)+'\n')
  print(label,code,row['matches_node'],protected,moved,flush=True)
assert len(record['rows'])==6
if arm=='candidate':
 assert all(r['exit_code']==0 and r['matches_node'] for r in record['rows']), 'Candidate remainder behavior failed; all outcomes preserved.'
 assert all(r['protected_retired_sets']>0 and r['moved_objects']>0 for r in record['rows'] if r['mode'].startswith('scheduled'))
else:
 print('Reference failures are preserved as findings, not accepted conformance passes.',flush=True)
