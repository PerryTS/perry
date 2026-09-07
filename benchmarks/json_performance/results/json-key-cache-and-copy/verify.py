#!/usr/bin/env python3
"""Verify both isolated changes and the complete measured comparisons."""
from pathlib import Path
import hashlib,json,statistics,subprocess
root=Path(__file__).resolve().parent;repo=root.parents[3]
read=lambda n:json.loads((root/n).read_text())
rows=lambda n:[json.loads(x) for x in (root/n).read_text().splitlines()]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for name,digest in read('manifest.json').items():assert sha(root/name)==digest,name
commits=read('source-commits.json')
for arm,expected_count in [('wide39',3290),('copy41',3292)]:
 p=read(arm+'/provenance.json');stamp=read(arm+'/source-stamp.json')
 assert p['release_matches_dist'] and stamp==p['source_sha256']
 for name,profile in p['effective_manifest_profiles'].items():assert profile['codegen-units']==(16 if name.endswith('-static') else 1)
 for name,digest in stamp.items():
  data=subprocess.check_output(['git','show',commits[arm]+':'+name],cwd=repo)
  assert hashlib.sha256(data).hexdigest()==digest,(arm,name)
 assert f'{expected_count} passed; 0 failed; 4 ignored' in (root/arm/'tests.log').read_text()
 a=read(arm+'/acceptance.json');assert len(a)==40 and all(r['matches_node'] for r in a)
 seeded=0
 for suffix in ['all-gc-stress','extended-validation','forced-tape-validation','no-json-growth-validation','lifetime-fixture-validation','root-record-validation']:
  for r in read(arm+'/'+suffix+'.json'):
   assert r['exit']==0 and r.get('live_subject',True)
   assert r.get('matches_node') or (r.get('matches_reference') and r.get('known_preexisting_mismatch_only'))
   c=r.get('counters',{})
   if c.get('scheduled_collections',0):
    assert all(c[k]>0 for k in ['scheduled_collections','copying_minors','moved_objects','loop_polls']);seeded+=1
 assert seeded==52
 expected=['mod','parser'] if arm=='wide39' else ['mod','stringify_flat','stringify_copy','stringify_copy_tests']
 base=commits['fused35' if arm=='wide39' else 'wide39']
 changed=subprocess.check_output(['git','diff','--name-only',base,commits[arm],'--','crates'],cwd=repo,text=True).splitlines()
 assert set(changed)=={'crates/perry-runtime/src/json/'+n+'.rs' for n in expected}
failed='launch40/results/'
assert not read(failed+'qualification.json')['qualified']
assert [len(rows(failed+n)) for n in ['recheck/verify.jsonl','recheck/timing.jsonl','recheck/memory.jsonl','lifetimes/raw.jsonl']]==[228,1140,480,84]
obs=read(failed+'trace-observations.json');assert len(obs)==76 and sum(bool(r['xprotect_busy']) for r in obs)==1
q=read('qualification.json');base=q['window']+'/results/'
obs=read(base+'trace-observations.json');assert len(obs)==q['observations']
assert all(r['monitor_exit']==0 and not r['external'] and not r['xprotect_busy'] for r in obs)
w=read(base+'window-custom.json');assert w['quiet_gate_passed'] and max(w['load_before'][0],w['load_after'][0])<=2.5
v=rows(base+'recheck/verify.jsonl');t=rows(base+'recheck/timing.jsonl');m=rows(base+'recheck/memory.jsonl');l=rows(base+'lifetimes/raw.jsonl')
assert len(v)==266 and all(r['correct'] for r in v)
assert (len(t),len(m),len(l))==(1330,600,105)
assert all(r['exit_code']==0 and 'error' not in r and r['tape'] is None for r in t+m)
assert all(not r['trace'] for r in l)
assert all(r['output_verified'] for r in l if r['mode'] in ['latest','retain'])
assert len({r['argv0'] for r in t+m+l if r['engine'] not in ['node','bun']})==1
meta=read(base+'recheck/metadata.json');lm=read(base+'lifetimes/metadata.json')
for e,arm in [('wide','wide39'),('candidate','copy41')]:
 p=read(arm+'/provenance.json')
 assert meta['hashes'][e]==p['workers']['worker']['sha256']
 assert lm['workers'][e]==p['workers']['lifetime-worker']['sha256']
s=read(base+'recheck/summary.json');assert len(s)==38
engines=['fused','previous','scalar','wide','candidate','node','bun']
for r in s:
 for e in engines:
  chosen=[x for x in t if (x['fixture'],x['operation'],x['engine'])==(r['fixture'],r['operation'],e)];assert len(chosen)==5
  assert statistics.median(x['cpu_ms']*1000/x['iterations'] for x in chosen)==r['cpu_us'][e]['median']
  assert statistics.median(x['peak_rss']/1048576 for x in chosen)==r['peak_rss_mib'][e]['median']
for metric in ['cpu_us','peak_rss_mib']:
 for e in ['wide','candidate']:
  assert sum(r[metric][e]['median']<=min(r[metric][other]['median'] for other in ['node','bun']) for r in s)==q[e][metric+'_rows_at_or_below_both']
for r in read(base+'recheck/memory-summary.json'):
 chosen=[x for x in m if (x['fixture'],x['operation'],x['iterations'])==(r['fixture'],r['operation'],r['iterations'])];assert len(chosen)==15
 for e in engines[:5]:
  for metric in ['peak_rss','rss_after']:assert statistics.median(x[metric]/1048576 for x in chosen if x['engine']==e)==r['values'][e][metric]['median']
for r in read(base+'lifetimes/summary.json'):
 chosen=[x for x in l if (x['fixture'],x['operation'],x['mode'],x['iterations'])==(r['fixture'],r['operation'],r['mode'],r['iterations'])];assert len(chosen)==15
 for e in engines[:5]:
  for metric in ['totalCpuUs','peak_rss','rssDrained']:assert statistics.median(x[metric] for x in chosen if x['engine']==e)==r['values'][e][metric]['median']
h=read('historical-147-inventory.json')
assert h['comparisons']==len(h['cases'])==147 and len({tuple(c[-2:]) for c in h['cases']})==38 and len({c[0] for c in h['cases']})==11
for r in read('cpu-regressions.json'):
 a,b=r['candidate'],r['reference']
 expected=[{'fixture':x['fixture'],'operation':x['operation'],'cpu_pct':100*(x['cpu_us'][a]['median']/x['cpu_us'][b]['median']-1)} for x in s if x['cpu_us'][a]['range'][0]>x['cpu_us'][b]['range'][1]]
 assert expected==r['slower_separated']
profiles=read('profile43/metadata.json')
assert len(profiles)==4
for r in profiles:
 assert r['diagnostic_only'] and r['process_exit']==r['sample_exit']==0
 assert r['worker_sha256']==read(r['arm']+'/provenance.json')['workers']['worker']['sha256']
for r in read('profile43/traces.json'):
 events=rows('profile43/'+r['arm']+'-wide.trace.jsonl')
 assert r['diagnostic_only'] and len(events)==r['cycles']==6
 assert sum(e['collection_kind']=='full' for e in events)==r['full']==1
 assert sum(e['layout_scans']['pointer_slots_read'] for e in events)==r['pointer_slots']==4212357
 assert r['worker_sha256']==read(r['arm']+'/provenance.json')['workers']['worker']['sha256']
assert not q['all_row_no_regression_goal_complete']
print('Both source changes, shipping profiles, correctness, rejected/qualified windows and all CPU/RSS/lifetime medians verified; goal remains open.')
