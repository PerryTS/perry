from pathlib import Path
import hashlib,json,subprocess,sys
w=Path(__file__).resolve().parent;root=w.parents[3];bench=w.parents[1]
index=bench/'results'/f'{w.name}-artifacts.json'
sha=lambda data:hashlib.sha256(data).hexdigest()
if '--write' in sys.argv:
 phases=[('regression-recheck-focus',528,60,0),('rotating',420,100,60),('korean-rotating',168,40,24)]
 validation=bench/'results'/(w.name+'-validation')
 paths=[bench/'DOMINANT_TOKEN_DISPATCH_R43.md']+[p for p in sorted(validation.rglob('*')) if p.is_file()]
 windows=[]
 timed=verify=calibration=0
 m=json.loads((w/'provenance.json').read_text())
 for suffix,nt,nv,nc in phases:
  d=bench/'results'/('quiet-'+w.name+'-'+suffix)
  assert json.loads((d/'controller-exit.json').read_text())['exit_code']==0
  window=json.loads((d/'window.json').read_text());assert window['quiet_gate_passed'] and window['finished_utc']
  assert len((d/'timing.jsonl').read_text().splitlines())==nt
  assert len((d/'verify.jsonl').read_text().splitlines())==nv
  if nc: assert len((d/'calibration.jsonl').read_text().splitlines())==nc
  else: assert not (d/'calibration.jsonl').exists()
  timed+=nt;verify+=nv;calibration+=nc
  paths += [p for p in sorted(d.rglob('*')) if p.is_file()]
  windows.append(dict(path=str(d.relative_to(root)),qualified=True,measured_source_commit=m['source_commit'],started_utc=window['started_utc'],finished_utc=window['finished_utc']))
 assert (timed,verify,calibration)==(1116,200,84)
 assert len(paths)==len(set(paths))
 focus=json.loads((w/'regression-recheck-analysis.json').read_text())['phases']['focus']
 rotating=json.loads((w/'rotating-analysis.json').read_text())
 korean=json.loads((w/'korean-rotating-analysis.json').read_text())
 cases=focus['cases']+rotating['cases']+korean['cases'];assert len(cases)==33
 regressions=sum(c['separated_regression'] for c in cases)
 data=dict(source_commit=m['source_commit'],reference_source_commit=m['main_build']['source_commit'],base_commit=m['base_commit'],timed_trials=timed,verification_records=verify,calibration_trials=calibration,archived_trial_counts=dict(timed=timed,verify=verify,calibration=calibration),windows=windows,qualification=f'Not promoted: {regressions} of 33 comparisons retain separated CPU regressions against R26. Three qualified windows cover twelve targeted repeated-input rows, fifteen fresh/repeated/select controls and six Korean controls. No full/options/retained/access-specific timing. Historical R42 vectors are not R43 trials. All 125 canonical candidate output checks pass with known findings explicit. Initial local setup and receipt-name refusals are preserved; no timing occurred in either refusal. Original150 staging hashes plus Korean assets verified. Not merged-main evidence.',files=[dict(path=str(p.relative_to(root)),bytes=p.stat().st_size,sha256=sha(p.read_bytes())) for p in paths])
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
