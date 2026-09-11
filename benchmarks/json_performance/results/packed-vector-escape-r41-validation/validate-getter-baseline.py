from pathlib import Path
import hashlib,json,os,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3];clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};source=w.with_name('materialized-read-r23')/'initial-validation/test_json_cached_reads.ts';records=[r for r in json.loads((w/'getter-baseline-comparison.json').read_text())['rows'] if r['arm']=='main']
node=subprocess.run(['/opt/homebrew/bin/node','--experimental-strip-types',str(source)],capture_output=True,env=clean,check=True);(w/'getter-probe-node.stdout').write_bytes(node.stdout)
for arm in ['candidate']:
 build=w/('frozen-main' if arm=='main' else 'frozen-build');binary=w/(arm+'-getter-probe');cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','-o',str(binary)]
 with (w/(arm+'-getter-probe-compile.log')).open('wb') as log:subprocess.run(cmd,cwd=root,env=clean|{'PERRY_RUNTIME_DIR':str(build)},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
 for mode,settings in [('auto',{}),('tape',{'PERRY_JSON_TAPE':'1'}),('direct',{'PERRY_JSON_TAPE':'0'})]:
  r=subprocess.run([str(binary)],env=clean|settings,capture_output=True,timeout=180);label=arm+'-getter-probe-'+mode
  (w/(label+'.stdout')).write_bytes(r.stdout);(w/(label+'.stderr')).write_bytes(r.stderr)
  if mode=='direct':assert r.returncode==0 and r.stdout==node.stdout
  else:assert r.returncode==1 and b"Cannot read properties of undefined" in r.stderr
  record={'arm':arm,'parser':mode,'exit_code':r.returncode,'matches_node':r.stdout==node.stdout,'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'compiler_sha256':hashlib.sha256((build/'perry').read_bytes()).hexdigest(),'runtime_sha256':hashlib.sha256((build/'libperry_runtime.a').read_bytes()).hexdigest(),'command':cmd};records.append(record);print(label,r.returncode,flush=True)
for mode in ['auto','tape','direct']:
 for ext in ['stdout','stderr']:assert (w/('main-getter-probe-'+mode+'.'+ext)).read_bytes()==(w/('candidate-getter-probe-'+mode+'.'+ext)).read_bytes()
(w/'getter-baseline-comparison.json').write_text(json.dumps({'rows':records,'note':'Existing lazy descriptor failures preserved unsuppressed; not a conformance pass.'},indent=2)+'\n')
