from pathlib import Path
import hashlib,json,os,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent;candidate='--candidate' in sys.argv;arm='candidate' if candidate else 'main';build=w/('frozen-build' if candidate else 'frozen-main');source=w.with_name('materialized-read-r23')/'lazy-space-probe.ts';binary=w/(arm+'-lazy-space-probe');clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','-o',str(binary)]
with (w/('lazy-probe-'+arm+'-compile.log')).open('wb') as log:subprocess.run(cmd,env=clean|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True)
records=[]
for count in [2,180]:
 for form in ['canonical','whitespace','duplicate']:
  for mode in ['plain','zero','true','pretty']:
   args=[str(count),form,mode];node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source)]+args,capture_output=True,env=clean,timeout=180);assert node.returncode==0
   r=subprocess.run([str(binary)]+args,capture_output=True,env=clean,timeout=180);label=('candidate-lazy-' if candidate else 'lazy-')+str(count)+'-'+form+'-'+mode
   (w/(label+'.stdout')).write_bytes(r.stdout);(w/(label+'.stderr')).write_bytes(r.stderr);(w/(label+'-node.stdout')).write_bytes(node.stdout)
   row={'count':count,'form':form,'mode':mode,'exit_code':r.returncode,'matches_node':r.stdout==node.stdout,'output_bytes':len(r.stdout)};records.append(row);print(row,flush=True)
(w/('lazy-candidate-probes.json' if candidate else 'lazy-main-probes.json')).write_text(json.dumps({'rows':records,'command':cmd,'files':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [source,binary,build/'perry',build/'libperry_runtime.a']}},indent=2)+'\n')

if candidate:
 base=json.loads((w/'lazy-main-probes.json').read_text())['rows'];assert len(base)==len(records)
 for a,b in zip(records,base):
  assert (a['count'],a['form'],a['mode'])==(b['count'],b['form'],b['mode'])
  assert a['exit_code']==b['exit_code'] and a['matches_node']==b['matches_node'],(a,b)
  stem=str(a['count'])+'-'+a['form']+'-'+a['mode']
  assert (w/('candidate-lazy-'+stem+'.stdout')).read_bytes()==(w/('lazy-'+stem+'.stdout')).read_bytes(),(a,b)
 print('PASS all24 baseline outcomes preserved, including separately recorded failures; not a conformance pass',flush=True)
