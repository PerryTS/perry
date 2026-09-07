#!/usr/bin/env python3
"""Verify isolated source changes, shipping builds, semantics and measurements."""
from pathlib import Path
import hashlib,json,statistics,subprocess
root=Path(__file__).resolve().parent;repo=root.parents[3]
read=lambda n:json.loads((root/n).read_text())
rows=lambda n:[json.loads(x) for x in (root/n).read_text().splitlines()]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for name,digest in read('manifest.json').items():assert sha(root/name)==digest,name
commits=read('source-commits.json')
for arm in commits:
    p=read(arm+'/provenance.json');stamp=read(arm+'/source-stamp.json')
    assert p['release_matches_dist'] and stamp==p['source_sha256']
    for name,profile in p['effective_manifest_profiles'].items():
        assert profile['codegen-units']==(16 if name.endswith('-static') else 1)
    for name,digest in stamp.items():
        data=subprocess.check_output(['git','show',commits[arm]+':'+name],cwd=repo)
        assert hashlib.sha256(data).hexdigest()==digest,(arm,name)
for arm,count in [('branch44',3292),('keys45',3295),('typed48',3296)]:
    assert f'{count} passed; 0 failed; 4 ignored' in (root/arm/'unit.log').read_text()
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
expected={'typed48':{'crates/perry-runtime/src/json/parse_api.rs','crates/perry-runtime/src/gc/tests/runtime_roots/json_key_lifetime.rs'},'branch44':{'crates/perry-runtime/src/json/parser.rs'},
          'keys45':{'crates/perry-runtime/src/json/mod.rs','crates/perry-runtime/src/gc/tests/runtime_roots.rs','crates/perry-runtime/src/gc/tests/runtime_roots/json_key_lifetime.rs'}}
for arm,base in [('branch44','copy41'),('keys45','branch44'),('typed48','keys45')]:
    changed=subprocess.check_output(['git','diff','--name-only',commits[base],commits[arm],'--','crates'],cwd=repo,text=True).splitlines()
    assert set(changed)==expected[arm]
before=(root/'keys45/before.log').read_text()
assert 'FAILED' in before and 'left: 2400016' in before and 'right: 0' in before
assert '3 passed; 0 failed' in (root/'keys45/focused.log').read_text()
assert '4 passed; 0 failed' in (root/'typed48/focused.log').read_text()
assert 'SIGBUS' in (root/'typed48/before.log').read_text()
for name in ['file-size.log','address-inventory.log']:
    assert (root/'keys45'/name).read_bytes()==(root/'copy41'/name).read_bytes()
assert 'gc_runtime_root_holders: OK' in (root/'keys45/root-inventory.log').read_text()
assert 'SIGBUS' in (root/'keys45/typed48-before.log').read_text()
for failed,counts,observations in [
    ('launch46/results/',[266,1330,600,150],104),
    ('launch49/results/',[304,1520,720,180],119),
]:
    assert not read(failed+'qualification.json')['qualified']
    assert [len(rows(failed+n)) for n in ['recheck/verify.jsonl','recheck/timing.jsonl','recheck/memory.jsonl','lifetimes/raw.jsonl']]==counts
    assert len(read(failed+'trace-observations.json'))==observations
q=read('qualification.json');assert q['previous_correctness_regression'];base=q['window']+'/results/'
obs=read(base+'trace-observations.json');assert len(obs)==q['observations']
assert all(r['monitor_exit']==0 and not r['external'] and not r['xprotect_busy'] for r in obs)
w=read(base+'window-custom.json');assert w['quiet_gate_passed'] and max(w['load_before'][0],w['load_after'][0])<=2.5
v=rows(base+'recheck/verify.jsonl');t=rows(base+'recheck/timing.jsonl');m=rows(base+'recheck/memory.jsonl');l=rows(base+'lifetimes/raw.jsonl')
assert len(v)==304 and all(r['correct'] for r in v)
assert (len(t),len(m),len(l))==(1520,720,180)
assert all(r['exit_code']==0 and 'error' not in r and r['tape'] is None for r in t+m)
assert all(not r['trace'] for r in l)
assert all(r['output_verified'] for r in l if r['mode'] in ['latest','retain'])
assert len({r['argv0'] for r in t+m+l if r['engine'] not in ['node','bun']})==1
meta=read(base+'recheck/metadata.json');lm=read(base+'lifetimes/metadata.json')
role_arms={'fused':'fused35','previous':'depth22','copy':'copy41','branch':'branch44','keys':'keys45','candidate':'typed48'}
for e,arm in role_arms.items():
    p=read(arm+'/provenance.json')
    assert meta['hashes'][e]==p['workers']['worker']['sha256']
    assert lm['workers'][e]==p['workers']['lifetime-worker']['sha256']
s=read(base+'recheck/summary.json');assert len(s)==38
engines=list(role_arms)+['node','bun']
for r in s:
    for e in engines:
        chosen=[x for x in t if (x['fixture'],x['operation'],x['engine'])==(r['fixture'],r['operation'],e)];assert len(chosen)==5
        assert statistics.median(x['cpu_ms']*1000/x['iterations'] for x in chosen)==r['cpu_us'][e]['median']
        assert statistics.median(x['peak_rss']/1048576 for x in chosen)==r['peak_rss_mib'][e]['median']
for metric in ['cpu_us','peak_rss_mib']:
    for e in ['branch','candidate']:
        assert sum(r[metric][e]['median']<=min(r[metric][o]['median'] for o in ['node','bun']) for r in s)==q[e][metric+'_rows_at_or_below_both']
ms=read(base+'recheck/memory-summary.json');assert len(ms)==40
for r in ms:
    chosen=[x for x in m if (x['fixture'],x['operation'],x['iterations'])==(r['fixture'],r['operation'],r['iterations'])];assert len(chosen)==18
    for e in role_arms:
        for metric in ['peak_rss','rss_after']:
            assert statistics.median(x[metric]/1048576 for x in chosen if x['engine']==e)==r['values'][e][metric]['median']
ls=read(base+'lifetimes/summary.json');assert len(ls)==10
assert {(r['mode'],r['iterations']) for r in ls if r['fixture']=='wide_1m'}=={('discard',100),('latest',100),('retain',16)}
for r in ls:
    chosen=[x for x in l if (x['fixture'],x['operation'],x['mode'],x['iterations'])==(r['fixture'],r['operation'],r['mode'],r['iterations'])];assert len(chosen)==18
    for e in role_arms:
        for metric in ['totalCpuUs','peak_rss','rssDrained']:
            assert statistics.median(x[metric] for x in chosen if x['engine']==e)==r['values'][e][metric]['median']
for r in read('cpu-regressions.json'):
    a,b=r['candidate'],r['reference']
    expected=[{'fixture':x['fixture'],'operation':x['operation'],'cpu_pct':100*(x['cpu_us'][a]['median']/x['cpu_us'][b]['median']-1)} for x in s if x['cpu_us'][a]['range'][0]>x['cpu_us'][b]['range'][1]]
    assert expected==r['slower_separated']
traces=read('trace50/metadata.json');assert len(traces)==3
for r in traces:
    events=rows('trace50/'+r['arm']+'-wide.trace.jsonl')
    assert r['diagnostic_only'] and r['process_exit']==0 and r['iterations']==36 and r['warmup']==2
    assert len(events)==r['cycles']
    assert sum(e['collection_kind']=='full' for e in events)==r['full']
    assert sum(e['layout_scans']['pointer_slots_read'] for e in events)==r['pointer_slots']
    assert events[-1]['arena_bytes']['after']['longlived']['in_use_bytes']==r['last_longlived_bytes']
    assert r['worker_sha256']==read(r['arm']+'/provenance.json')['workers']['worker']['sha256']
h=read('historical-147-inventory.json')
assert h['comparisons']==len(h['cases'])==147 and len({tuple(c[-2:]) for c in h['cases']})==38 and len({c[0] for c in h['cases']})==11
assert not q['all_row_no_regression_goal_complete'] and not q['promoted_over_depth22']
print('Source pins, matched shipping profiles, correctness, all38 CPU/RSS, 40 retained groups and 10 lifetime cases verified; goal remains open.')
