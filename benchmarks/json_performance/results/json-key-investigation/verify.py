#!/usr/bin/env python3
"""Verify historical experiments, keeping raw evidence separate from live source."""
from pathlib import Path
import hashlib,json,statistics,subprocess
root=Path(__file__).resolve().parent;repo=root.parents[3]
read=lambda n:json.loads((root/n).read_text())
rows=lambda n:[json.loads(x) for x in (root/n).read_text().splitlines()]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for name,digest in read('manifest.json').items():assert sha(root/name)==digest,name
commits=read('source-commits.json')
for arm,commit in commits.items():
 p=read(arm+'/provenance.json');stamp=read(arm+'/source-stamp.json')
 assert p['release_matches_dist'] and stamp==p['source_sha256']
 for name,profile in p['effective_manifest_profiles'].items():assert profile['codegen-units']==(16 if name.endswith('-static') else 1)
 for name,digest in stamp.items():
  data=subprocess.check_output(['git','show',commit+':'+name],cwd=repo)
  assert hashlib.sha256(data).hexdigest()==digest,(arm,name)
 count=3288 if arm=='scalar31' else 3290
 assert f'{count} passed; 0 failed; 4 ignored' in (root/arm/'tests.log').read_text()
 a=read(arm+'/acceptance.json');assert len(a)==40 and all(r['matches_node'] for r in a)
 seeded=0
 for suffix in ['all-gc-stress','extended-validation','forced-tape-validation','no-json-growth-validation','lifetime-fixture-validation','root-record-validation']:
  for r in read(arm+'/'+suffix+'.json'):
   assert r['exit']==0 and r.get('live_subject',True),(arm,suffix)
   assert r.get('matches_node',False) or (r.get('matches_reference',False) and r.get('known_preexisting_mismatch_only',False)),(arm,suffix)
   c=r.get('counters',{})
   if c.get('scheduled_collections',0):
    assert all(c[k]>0 for k in ['scheduled_collections','copying_minors','moved_objects','loop_polls']);seeded+=1
 assert seeded==52,(arm,seeded)
for window,extra,arm in [('launch32','structural','scalar31'),('launch34','scalar','key33')]:
 base=window+'/results/'
 obs=read(base+'trace-observations.json');assert len(obs)==77
 assert all(r['monitor_exit']==0 and not r['external'] and not r['xprotect_busy'] for r in obs)
 w=read(base+'window-custom.json');assert w['quiet_gate_passed'] and max(w['load_before'][0],w['load_after'][0])<=2.5
 v=rows(base+'recheck/verify.jsonl');t=rows(base+'recheck/timing.jsonl');m=rows(base+'recheck/memory.jsonl');l=rows(base+'lifetimes/raw.jsonl')
 assert len(v)==228 and all(r['correct'] for r in v)
 assert (len(t),len(m),len(l))==(1140,480,84)
 assert all(r['exit_code']==0 and 'error' not in r and r['tape'] is None for r in t+m)
 assert all(not r['trace'] for r in l)
 assert all(r['output_verified'] for r in l if r['mode'] in ['latest','retain'])
 assert len({r['argv0'] for r in t+m+l if r['engine'] not in ['node','bun']})==1
 p=read(arm+'/provenance.json');meta=read(base+'recheck/metadata.json');lm=read(base+'lifetimes/metadata.json')
 assert meta['hashes']['candidate']==p['workers']['worker']['sha256']
 assert lm['workers']['candidate']==p['workers']['lifetime-worker']['sha256']
 s=read(base+'recheck/summary.json');assert len(s)==38
 engines=['checkpoint','previous',extra,'candidate','node','bun']
 for r in s:
  for e in engines:
   chosen=[x for x in t if (x['fixture'],x['operation'],x['engine'])==(r['fixture'],r['operation'],e)];assert len(chosen)==5
   assert statistics.median(x['cpu_ms']*1000/x['iterations'] for x in chosen)==r['cpu_us'][e]['median']
   assert statistics.median(x['peak_rss']/1048576 for x in chosen)==r['peak_rss_mib'][e]['median']
 for metric in ['cpu_us','peak_rss_mib']:
  assert sum(r[metric]['candidate']['median']<=min(r[metric][e]['median'] for e in ['node','bun']) for r in s)==(17 if metric=='cpu_us' else 32)
 for r in read(base+'recheck/memory-summary.json'):
  chosen=[x for x in m if (x['fixture'],x['operation'],x['iterations'])==(r['fixture'],r['operation'],r['iterations'])];assert len(chosen)==12
  for e in engines[:4]:
   for metric in ['peak_rss','rss_after']:assert statistics.median(x[metric]/1048576 for x in chosen if x['engine']==e)==r['values'][e][metric]['median']
 for r in read(base+'lifetimes/summary.json'):
  chosen=[x for x in l if (x['fixture'],x['operation'],x['mode'],x['iterations'])==(r['fixture'],r['operation'],r['mode'],r['iterations'])];assert len(chosen)==12
  for e in engines[:4]:
   for metric in ['totalCpuUs','peak_rss','rssDrained']:assert statistics.median(x[metric] for x in chosen if x['engine']==e)==r['values'][e][metric]['median']
print('Historical source, validation, qualification and all CPU/RSS/lifetime medians verified; neither experiment is promoted.')
