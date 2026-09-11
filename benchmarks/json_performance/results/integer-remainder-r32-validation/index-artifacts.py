from pathlib import Path
import hashlib,json,subprocess,sys
w=Path(__file__).resolve().parent;root=w.parents[3];bench=w.parents[1]
index=bench/'results'/f'{w.name}-artifacts.json'
sha=lambda data:hashlib.sha256(data).hexdigest()
if '--write' in sys.argv:
 dirs=sorted((bench/'results').glob('quiet-'+w.name+'-*'))+[bench/'results'/(w.name+'-validation')]
 paths=[bench/'INTEGER_REMAINDER_R32.md']+[p for d in dirs for p in sorted(d.rglob('*')) if p.is_file()]
 assert len(paths)==len(set(paths))
 timed=verify=calibration=0
 for d in dirs[:-1]:
  assert json.loads((d/'controller-exit.json').read_text())['exit_code']==0
  for name,kind in [('timing.jsonl','timed'),('memory.jsonl','timed'),('verify.jsonl','verify'),('calibration.jsonl','calibration')]:
   if (d/name).exists():
    n=len((d/name).read_text().splitlines())
    if kind=='timed':timed+=n
    elif kind=='verify':verify+=n
    else:calibration+=n
 m=json.loads((w/'provenance.json').read_text())
 data=dict(source_commit=m['source_commit'],reference_source_commit=m['main_build']['source_commit'],base_commit=m['base_commit'],timed_trials=timed,verification_records=verify,calibration_trials=calibration,qualification='Controlled R26 comparison at 0.5.1531. All timing tradeoffs and validation limitations are disclosed in the report; not merged-main evidence.',files=[dict(path=str(p.relative_to(root)),bytes=p.stat().st_size,sha256=sha(p.read_bytes())) for p in paths])
 index.write_text(json.dumps(data,indent=2)+'\n')
 print('INDEXED',len(paths),'payloads;',timed,'timed,',verify,'verify,',calibration,'calibration')
data=json.loads(index.read_text());entries=data['files']+[dict(path=str(index.relative_to(root)),bytes=index.stat().st_size,sha256=sha(index.read_bytes()))]
for e in entries:
 b=(root/e['path']).read_bytes();assert len(b)==e['bytes'] and sha(b)==e['sha256'],e['path']
if '--stage' in sys.argv:
 paths=[e['path'] for e in entries]
 # Only these indexed evidence files may be force-added despite trace/worker ignore rules.
 for i in range(0,len(paths),200):subprocess.run(['git','add','-f','--',*paths[i:i+200]],cwd=root,check=True)
for mode in ['stage','commit']:
 if '--'+mode not in sys.argv:continue
 prefix=':' if mode=='stage' else 'HEAD:'
 raw=subprocess.check_output(['git','cat-file','--batch'],cwd=root,input=''.join(prefix+e['path']+'\n' for e in entries).encode());at=0
 for e in entries:
  end=raw.index(b'\n',at);header=raw[at:end].split();assert len(header)==3 and header[1]==b'blob',e['path'];size=int(header[2]);start=end+1;b=raw[start:start+size];at=start+size+1
  assert size==e['bytes'] and sha(b)==e['sha256'],(mode,e['path'])
 assert at==len(raw)
 print('VERIFIED',mode,len(entries),'Git blobs')
