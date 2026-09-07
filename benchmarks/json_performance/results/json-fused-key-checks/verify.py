#!/usr/bin/env python3
"""Check source, build profiles, correctness, window qualification and raw medians."""
from pathlib import Path
import hashlib,json,statistics,subprocess
root=Path(__file__).resolve().parent;repo=root.parents[3]
read=lambda n:json.loads((root/n).read_text())
rows=lambda n:[json.loads(x) for x in (root/n).read_text().splitlines()]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for name,digest in read('manifest.json').items():assert sha(root/name)==digest,name
commit=read('source-commit.json');p=read('provenance.json');stamp=read('source-stamp.json')
assert p['release_matches_dist'] and stamp==p['source_sha256']
for name,profile in p['effective_manifest_profiles'].items():assert profile['codegen-units']==(16 if name.endswith('-static') else 1)
for name,digest in stamp.items():
 data=subprocess.check_output(['git','show',commit['candidate']+':'+name],cwd=repo)
 assert hashlib.sha256(data).hexdigest()==digest,name
changed=subprocess.check_output(['git','diff','--name-only',commit['base'],commit['candidate'],'--','crates'],cwd=repo,text=True).splitlines()
assert set(changed)=={'crates/perry-runtime/src/json/'+n+'.rs' for n in ['stringify_flat','stringify_record_output','stringify_record_output_tests','stringify_tojson_probe','stringify_tojson_probe_tests']}
assert '3290 passed; 0 failed; 4 ignored' in (root/'tests.log').read_text()
a=read('acceptance.json');assert len(a)==40 and all(r['matches_node'] for r in a)
seeded=0
for suffix in ['all-gc-stress','extended-validation','forced-tape-validation','no-json-growth-validation','lifetime-fixture-validation','root-record-validation']:
 for r in read(suffix+'.json'):
  assert r['exit']==0 and r.get('live_subject',True)
  assert r.get('matches_node') or (r.get('matches_reference') and r.get('known_preexisting_mismatch_only'))
  c=r.get('counters',{})
  if c.get('scheduled_collections',0):
   assert all(c[k]>0 for k in ['scheduled_collections','copying_minors','moved_objects','loop_polls']);seeded+=1
assert seeded==52
q=read('qualification.json');base=q['window']+'/results/'
obs=read(base+'trace-observations.json');assert len(obs)==q['observations']
assert all(r['monitor_exit']==0 and not r['external'] and not r['xprotect_busy'] for r in obs)
w=read(base+'window-custom.json');assert w['quiet_gate_passed'] and max(w['load_before'][0],w['load_after'][0])<=2.5
v=rows(base+'recheck/verify.jsonl');t=rows(base+'recheck/timing.jsonl');m=rows(base+'recheck/memory.jsonl');l=rows(base+'lifetimes/raw.jsonl')
assert len(v)==228 and all(r['correct'] for r in v)
assert (len(t),len(m),len(l))==(1140,480,84)
assert all(r['exit_code']==0 and 'error' not in r and r['tape'] is None for r in t+m)
assert all(not r['trace'] for r in l)
assert all(r['output_verified'] for r in l if r['mode'] in ['latest','retain'])
assert len({r['argv0'] for r in t+m+l if r['engine'] not in ['node','bun']})==1
meta=read(base+'recheck/metadata.json');lm=read(base+'lifetimes/metadata.json')
assert meta['hashes']['candidate']==p['workers']['worker']['sha256']
assert lm['workers']['candidate']==p['workers']['lifetime-worker']['sha256']
prior=json.loads((root.parent/'json-key-investigation/scalar31/provenance.json').read_text())
assert meta['hashes']['scalar']==prior['workers']['worker']['sha256']
assert lm['workers']['scalar']==prior['workers']['lifetime-worker']['sha256']
s=read(base+'recheck/summary.json');assert len(s)==38
engines=['checkpoint','previous','scalar','candidate','node','bun']
for r in s:
 for e in engines:
  chosen=[x for x in t if (x['fixture'],x['operation'],x['engine'])==(r['fixture'],r['operation'],e)];assert len(chosen)==5
  assert statistics.median(x['cpu_ms']*1000/x['iterations'] for x in chosen)==r['cpu_us'][e]['median']
  assert statistics.median(x['peak_rss']/1048576 for x in chosen)==r['peak_rss_mib'][e]['median']
 for ref in ['previous','scalar']:
  assert r['comparisons'][ref]['cpu_pct']==100*(r['cpu_us']['candidate']['median']/r['cpu_us'][ref]['median']-1)
  assert r['comparisons'][ref]['rss_kib']==1024*(r['peak_rss_mib']['candidate']['median']-r['peak_rss_mib'][ref]['median'])
for metric in ['cpu_us','peak_rss_mib']:
 assert sum(r[metric]['candidate']['median']<=min(r[metric][e]['median'] for e in ['node','bun']) for r in s)==q[metric+'_rows_at_or_below_both']
for r in read(base+'recheck/memory-summary.json'):
 chosen=[x for x in m if (x['fixture'],x['operation'],x['iterations'])==(r['fixture'],r['operation'],r['iterations'])];assert len(chosen)==12
 for e in engines[:4]:
  for metric in ['peak_rss','rss_after']:assert statistics.median(x[metric]/1048576 for x in chosen if x['engine']==e)==r['values'][e][metric]['median']
for r in read(base+'lifetimes/summary.json'):
 chosen=[x for x in l if (x['fixture'],x['operation'],x['mode'],x['iterations'])==(r['fixture'],r['operation'],r['mode'],r['iterations'])];assert len(chosen)==12
 for e in engines[:4]:
  for metric in ['totalCpuUs','peak_rss','rssDrained']:assert statistics.median(x[metric] for x in chosen if x['engine']==e)==r['values'][e][metric]['median']
assert not q['all_row_no_regression_goal_complete']
diag='diag38/results/'
assert read(diag+'window-custom.json')['quiet_gate_passed']
observations=read(diag+'trace-observations.json')
assert len(observations)==15
assert all(r['monitor_exit']==0 and not r['external'] and not r['xprotect_busy'] for r in observations)
assert read(diag+'metadata.json')['hashes']=={e:meta['hashes'][e] for e in ['previous','scalar','candidate']}
targeted=rows(diag+'timing.jsonl')
assert len(targeted)==63 and all(r['exit_code']==0 and not r['trace'] and 'error' not in r for r in targeted)
for r in read(diag+'summary.json'):
 for e in ['previous','scalar','candidate']:
  chosen=[x for x in targeted if (x['fixture'],x['engine'])==(r['fixture'],e)]
  assert len(chosen)==7
  assert statistics.median(x['cpu_ms']*1000/x['iterations'] for x in chosen)==r['cpu_us'][e]['median']
 for ref in ['previous','scalar']:
  c=r['cpu_us'];v=r['comparisons'][ref]
  assert v['cpu_pct']==100*(c['candidate']['median']/c[ref]['median']-1)
  assert v['slower_ranges_separated']==(c['candidate']['range'][0]>c[ref]['range'][1])
print('Fused key checks: source, matched build, correctness, GC stress and all 38 CPU/RSS rows verified. Overall goal remains open.')
