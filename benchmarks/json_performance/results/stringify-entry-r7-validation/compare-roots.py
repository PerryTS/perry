from pathlib import Path
import hashlib,json,re,subprocess
from concurrent.futures import ThreadPoolExecutor
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent;r5=w.parent/'invariant-field-loop-r5'
records=[]
subjects=[('invariant',list((r5/'main-fixture-native').glob('*.ll')),list((w/'ir-native').glob('test_json_invariant_field_loop-*.ll'))),('projection',list((r5/'main-current-projection').glob('*.ll')),list((w/'ir-native').glob('test_json_scalar_projection-*.ll'))),('entry',list((w/'ir-main-entry').glob('worker-*.ll')),list((w/'ir-native').glob('test_json_stringify_entry-*.ll'))),('options',list((w/'ir-r5-options').glob('worker-*.ll')),list((w/'ir-candidate-options').glob('worker-*.ll')))]
def check_subject(subject):
 label,baseline,candidate=subject
 verdicts=[]
 for arm,paths in [('baseline',baseline),('candidate',candidate)]:
  assert paths,(label,arm)
  cmd=['python3',str(root/'scripts/gc_root_dominance_check.py'),'--statepoints','--max-stale','0','--min-files','1','--min-statepoints','1','--min-live-bundles','1','--min-relocates','1']+[str(p) for p in paths]
  r=subprocess.run(cmd,cwd=root,capture_output=True,text=True);log=w/(label+'-'+arm+'-roots.log');log.write_text(r.stdout+r.stderr)
  fingerprints=sorted(re.findall(r'fingerprint\s*:\s*[^:\n]+::([^\n]+)',r.stdout));count=int(re.search(r'statepoint hazards: (\d+)',r.stdout).group(1));assert len(fingerprints)==count,(label,arm,fingerprints,count)
  verdicts.append({'arm':arm,'exit_code':r.returncode,'fingerprints':fingerprints,'count':count,'log':str(log.relative_to(w)),'files':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}})
 same=verdicts[0]['fingerprints']==verdicts[1]['fingerprints']
 print('ROOT FINGERPRINTS',label,same,verdicts[0]['count'],flush=True)
 return {'subject':label,'same':same,'verdicts':verdicts}
with ThreadPoolExecutor(max_workers=4) as pool:records=list(pool.map(check_subject,subjects))
(w/'root-fingerprint-comparison.json').write_text(json.dumps(records,indent=2)+'\n');assert all(r['same'] for r in records)
