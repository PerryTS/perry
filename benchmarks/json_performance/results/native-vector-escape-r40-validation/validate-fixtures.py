from pathlib import Path
import hashlib,json,os,re,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
main='--main' in sys.argv;arm='main' if main else 'candidate';build=w/('frozen-main' if main else 'frozen-build');clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};records=[]
stress={'PERRY_GC_SCHEDULE_SEED':'10022','PERRY_GC_SCHEDULE_RATE':'0.1','PERRY_GC_SCHEDULE_ALLOC_KB':'0','PERRY_GC_PROTECT_FROMSPACE':'1','PERRY_GC_DIAG':'1'}
subjects=['lazy-spacer'] if '--lazy-only' in sys.argv else ['test_json_source_length','test_json_template_capture','test_json_cached_construction','test_json_cached_reads','prior-entry','callback-only']
for stem in subjects:
 source=w.with_name('materialized-read-r23')/(stem+'.ts');binary=w/(arm+'-'+stem)
 node=subprocess.run(['/opt/homebrew/bin/node','--expose-gc','--experimental-strip-types',str(source)],env=clean,capture_output=True,timeout=180);assert node.returncode==0,node.stderr
 (w/(stem+'-node.stdout')).write_bytes(node.stdout)
 cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','-o',str(binary)]
 with (w/(arm+'-'+stem+'-compile.log')).open('wb') as log:subprocess.run(cmd,env=clean|{'PERRY_RUNTIME_DIR':str(build)},cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 for parser,settings in [('auto',{}),('tape',{'PERRY_JSON_TAPE':'1'}),('direct',{'PERRY_JSON_TAPE':'0'})]:
  for mode,knobs in [('normal',{}),('scheduled',stress),('fullgc',{'PERRY_GEN_GC':'0'})]:
   if stem=='callback-only' and (parser!='auto' or mode!='scheduled'):continue
   label=arm+'-'+stem+'-'+parser+'-'+mode;r=subprocess.run([str(binary)],env=clean|settings|knobs,capture_output=True,timeout=180)
   (w/(label+'.stdout')).write_bytes(r.stdout);(w/(label+'.stderr')).write_bytes(r.stderr)
   diag=r.stderr.decode(errors='replace');protected=len(re.findall(r'\[gc-fromspace-protect\].*retired_set=#',diag));moved=sum(sum(map(int,re.findall(r'\b(?:copied_objects|promoted_objects)=(\d+)',line))) for line in diag.splitlines() if line.startswith('[gc-copy-minor] ran'))
   row={'subject':stem,'parser':parser,'gc':mode,'matches_node':r.stdout==node.stdout,'exit_code':r.returncode,'protected_retired_sets':protected,'moved_objects':moved,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'command':cmd};records.append(row);print(label,r.returncode,row['matches_node'],protected,moved,flush=True)
   assert r.returncode==0 and row['matches_node'],(row,r.stderr[-1000:],r.stdout[:200],node.stdout[:200])
   if mode=='scheduled':assert protected>0 and moved>0,row
(w/(arm+('-lazy-validation.json' if '--lazy-only' in sys.argv else '-fixture-validation.json'))).write_text(json.dumps(records,indent=2)+'\n')
