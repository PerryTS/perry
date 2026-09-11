from pathlib import Path
import hashlib,json,os,re,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
references=[('main',w/'main-retained-zero')]
if '--candidate' in sys.argv:references=[('candidate',w/'candidate-retained-zero')]
cases=[(fixture,'retain-stringify') for fixture in ['tiny_object','small_record','long_string_1m','unicode_1m']]
records=[]
for fixture,operation in cases:
 args=[str(root/'benchmarks/json_performance/.work/fixtures'/(fixture+'.json')),operation,'64','8','verify']
 node=subprocess.run(['/opt/homebrew/bin/node',str(w/'retained-zero-worker.js')]+args,capture_output=True,env=clean,timeout=180)
 assert node.returncode==0,node.stderr
 def semantic(output):
  text=output.decode();result=next(line for line in text.splitlines() if line.startswith('RESULT ')).split();return result[6:8],text.split('\nVERIFY ',1)[1]
 expected=semantic(node.stdout)
 for arm,binary in references:
  for mode,knobs in [('normal',{}),('scheduled',{'PERRY_GC_SCHEDULE_SEED':'10022','PERRY_GC_SCHEDULE_RATE':'0.1','PERRY_GC_SCHEDULE_ALLOC_KB':'0','PERRY_GC_PROTECT_FROMSPACE':'1'})]:
   label='retained-zero-'+arm+'-'+fixture+'-'+operation+'-'+mode
   r=subprocess.run([str(binary)]+args,capture_output=True,env=clean|knobs|{'PERRY_GC_DIAG':'1'},timeout=180)
   (w/(label+'.stdout')).write_bytes(r.stdout);(w/(label+'.stderr')).write_bytes(r.stderr)
   assert r.returncode==0,(label,r.returncode,r.stderr[-2000:])
   matched=semantic(r.stdout)==expected
   diagnostic=r.stderr.decode(errors='replace');protected=len(re.findall(r'\[gc-fromspace-protect\].*retired_set=#',diagnostic));moved=sum(sum(map(int,re.findall(r'\b(?:copied_objects|promoted_objects)=(\d+)',line))) for line in diagnostic.splitlines() if line.startswith('[gc-copy-minor] ran'))
   row={'arm':arm,'fixture':fixture,'operation':operation,'mode':mode,'matches_node':matched,'protected_retired_sets':protected,'moved_objects':moved,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()};records.append(row);print(label,matched,protected,moved,flush=True)
   assert matched,(label,semantic(r.stdout),expected)
   if mode=='scheduled':assert protected>0 and moved>0,row
(w/('candidate-retained-zero-validation.json' if '--candidate' in sys.argv else 'main-retained-zero-validation.json')).write_text(json.dumps(records,indent=2)+'\n')
