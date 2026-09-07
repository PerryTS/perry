#!/usr/bin/env python3
"""Check saved measurements, validation, source identities and experiment status."""
from pathlib import Path
import hashlib,json,subprocess,statistics

root=Path(__file__).resolve().parent
repo=root.parents[3]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
read=lambda p:json.loads((root/p).read_text())
rows=lambda p:[json.loads(line) for line in (root/p).read_text().splitlines()]
q=read('qualification.json')
assert q['experimental'] and not q['no_regression_acceptance']
assert q['full_matrix_complete'] and not q['decode18_full_matrix_complete']
assert not q['gc_policy_changed']
for path,digest in read('manifest.json').items():
    assert sha(root/path)==digest,path

for arm in ['decode17','decode18']:
    stamp=read(arm+'/source-stamp.json')
    provenance=read(arm+'/provenance.json')
    assert stamp==provenance['source_sha256'] and len(stamp)==93
    assert provenance['release_matches_dist']
    for name,profile in provenance['effective_manifest_profiles'].items():
        assert profile['codegen-units']==(16 if name.endswith('-static') else 1)
    for path,digest in stamp.items():
        source=subprocess.check_output(['git','show',q['source_commits'][arm]+':'+path],cwd=repo)
        assert hashlib.sha256(source).hexdigest()==digest,(arm,path)
    assert '3283 passed; 0 failed; 4 ignored' in (root/arm/'tests.log').read_text()
    checks=read(arm+'/acceptance.json')
    assert len(checks)==40 and all(r['matches_node'] for r in checks)
    seeded=0
    for name in ['all-gc-stress','extended-validation','forced-tape-validation',
                 'no-json-growth-validation','lifetime-fixture-validation','root-record-validation']:
        for r in read(arm+'/validation/'+name+'.json'):
            assert r['exit']==0 and r.get('live_subject',True)
            assert r.get('matches_node') or (r.get('matches_reference') and r.get('known_preexisting_mismatch_only'))
            counters=r.get('counters',{})
            if counters.get('scheduled_collections',0):
                seeded+=1
                assert all(counters[k]>0 for k in ['scheduled_collections','copying_minors','moved_objects','loop_polls'])
    assert seeded==52,(arm,seeded)
current=read(q['runtime_candidate']+'/source-stamp.json')
assert all(sha(repo/path)==digest for path,digest in current.items())
before=subprocess.check_output(['git','show',q['source_commits']['decode17']+':crates/perry-runtime/src/json/parser.rs'],cwd=repo)
after=subprocess.check_output(['git','show',q['source_commits']['decode18']+':crates/perry-runtime/src/json/parser.rs'],cwd=repo)
needle=b'#[inline(always)]\n    pub(crate) fn parse_string_bytes('
assert before.count(needle)==1
assert before.replace(needle,needle.replace(b'always',b'never'))==after

for arm,observations in [('decode16',33),('decode17',170),('decode18',71),('decode17/profiles',4)]:
    p=arm+'/results/'
    window=read(p+'window-custom.json')
    assert window['quiet_gate_passed']
    assert max(window['load_before'][0],window['load_after'][0])<=2.5
    monitor=read(p+'trace-observations.json')
    assert len(monitor)==observations
    assert all(r['monitor_exit']==0 and not r['external'] for r in monitor)
assert not read('inode12/results/window-custom.json')['quiet_gate_passed']
assert 'AssertionError' in (root/'decode16/failed-hardlink-controller.log').read_text()

for folder,count in [('decode13',120),('decode14',252),('decode15',252)]:
    p='prototypes/'+folder+'/'
    data=read(p+'results/raw.json')
    assert len(data)==count
    for fixture in {r['fixture'] for r in data}:
        assert len({r['checksum'] for r in data if r['fixture']==fixture})==1
    assert read(p+'results/window-custom.json')['quiet_gate_passed']
    for path,digest in read(p+'source-provenance.json')['source_sha256'].items():
        assert sha(root/p/path)==digest,(folder,path)
    assert '2 passed; 0 failed' in (root/p/'test.log').read_text()
assert 'escaped=true changed=43' in (root/'prototypes/decode17-prototype/capacity_control.log').read_text()
assert 'escaped=true changed=0' in (root/'prototypes/decode17-prototype/capacity_fixed.log').read_text()
assert '2 passed; 0 failed' in (root/'prototypes/decode17-prototype/test.log').read_text()

for arm,phase,nverify,ntiming,nmemory,nlife in [
    ('decode16','focused',45,315,0,0),
    ('decode17','recheck',190,950,360,162),
    ('decode18','focused',54,378,0,147),
]:
    p=arm+'/results/'
    verify=rows(p+phase+'/verify.jsonl')
    assert len(verify)==nverify and all(r['correct'] for r in verify)
    timing=rows(p+phase+'/timing.jsonl')
    assert len(timing)==ntiming
    measured=timing[:]
    if nmemory:
        memory=rows(p+phase+'/memory.jsonl');assert len(memory)==nmemory
        measured+=memory
    assert all(r['exit_code']==0 and 'error' not in r and r['tape'] is None for r in measured)
    if nlife:
        lifetime=rows(p+'lifetimes/raw.jsonl');assert len(lifetime)==nlife
        assert all(not r['trace'] for r in lifetime)
        assert all(r['output_verified'] for r in lifetime if r['mode'] in ['retain','latest'])
        measured+=lifetime
    assert len({r['executable_path'] for r in measured if r['engine'] not in ['node','bun']})==1
    hashes=read(p+phase+'/metadata.json')['hashes']
    assert hashes['candidate']==read(arm+'/provenance.json')['workers']['worker']['sha256']
    if arm=='decode18':assert hashes['inline']==read('decode17/provenance.json')['workers']['worker']['sha256']

full=read('decode17/results/recheck/summary.json');assert len(full)==38
focused=read('decode18/results/focused/summary.json');assert len(focused)==9
for arm,phase,summary in [('decode17','recheck',full),('decode18','focused',focused)]:
    raw=rows(arm+'/results/'+phase+'/timing.jsonl')
    for r in summary:
        for engine in {x['engine'] for x in raw}:
            selected=[x for x in raw if (x['fixture'],x['operation'],x['engine'])==(r['fixture'],r['operation'],engine)]
            assert statistics.median(x['cpu_ms']*1000/x['iterations'] for x in selected)==r['cpu_us'][engine]['median']
            assert statistics.median(x['peak_rss']/1048576 for x in selected)==r['peak_rss_mib'][engine]['median']
for r in focused:
    if (r['fixture'],r['operation'])==('escaped_1m','parse'):
        assert -31.18<r['cpu_us']['delta_percent_vs_counter']<-31.17
        assert r['cpu_us']['node']['median']<r['cpu_us']['candidate']['median']<r['cpu_us']['bun']['median']
profile=read('decode17/profiles/results/summary.json')
assert len(profile)==6 and all(r['sample_exit']==0 and r['worker_exit']==-15 for r in profile)
assert {r['name']:r['sha256'] for r in read('decode17/fixture-check.json')}=={
    r['name']:r['sha256'] for r in read('decode17/results/fixtures.json')}
assert {r['check']:r['exit'] for r in read('decode17/checks.json')}=={
    'root-holders':0,'address-inventory':1,'node-consistency':0,'file-size':1,'diff':0}
print('Escaped decoder source, prototypes, correctness, all 38 rows, focused control and GC lifetimes verified; no regression acceptance remains open.')
