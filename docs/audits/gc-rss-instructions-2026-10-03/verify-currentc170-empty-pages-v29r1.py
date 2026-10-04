"""Independently bind empty-page observations to authoritative space census."""
import pathlib,json,hashlib,tarfile
B=pathlib.Path('/root/rss-header-20261002');V='gc-currentc170-empty-arena-pages-v29r1';O=B/V
def sha(p):
    with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert (B/(V+'-driver.exit')).read_text().strip()=='0'
assert (B/(V+'.exit')).read_text().strip()=='0'
runs=json.loads((B/(V+'-runs.json')).read_text());assert len(runs)==2
oracles=json.loads((B/'gc-currentc170-qb6-v22-noauto-oracle.json').read_text())
summary=[]
for r in runs:
    arm=r['arm'];prefix=O/f'{V}-{arm}-tscwork'
    assert r['rc']==0 and r['reason'] is None and prefix.with_suffix('.app.out').read_text()==oracles['tscwork']
    assert sha(prefix.with_suffix('.jsonl'))==r['census_sha256']
    assert sha(prefix.with_suffix('.boundaries.jsonl'))==r['boundaries_sha256']
    censuses=[json.loads(s) for s in prefix.with_suffix('.jsonl').read_text().splitlines()]
    pages=[json.loads(s) for s in (O/(arm+'.pages.jsonl')).read_text().splitlines()]
    assert len(pages)==len(censuses)==r['natural_synchronous_fulls']
    for i,(p,c) in enumerate(zip(pages,censuses)):
        assert p['full']==i+1 and c['seq']==i
        for s,a in zip(p['spaces'],c['arena']['spaces'][:5]):
            assert s['space']==a['space'] and s['used_bytes']==a['used_bytes'] and s['capacity_bytes']==a['capacity_bytes']
            assert sum(b['size'] for b in s['blocks'] if b['data'])==s['capacity_bytes']
            assert sum(b['offset'] for b in s['blocks'] if b['data'])==s['used_bytes']
            assert sum(bool(b['data']) for b in s['blocks'])==a['blocks']
            for b in s['blocks']:
                assert 0<=b['offset']<=b['size']
                assert b['interior_bytes']==max(0,((b['data']+b['size'])//4096-(b['data']+4095)//4096)*4096) if b['data'] else b['interior_bytes']==0
                assert 0<=b['resident_interior_bytes']<=b['interior_bytes']
                assert b['current']==(b['index']==s['current'])
    def point(i):
        p=pages[i]
        return dict(seq=i,spaces=[dict(space=s['space'],empty_noncurrent_blocks=sum(b['offset']==0 and b['data']!=0 and not b['current'] for b in s['blocks']),empty_noncurrent_resident_bytes=sum(b['resident_interior_bytes'] for b in s['blocks'] if b['offset']==0 and b['data'] and not b['current'])) for s in p['spaces']])
    summary.append(dict(arm=arm,observations=len(pages),first=point(0),last=point(len(pages)-1),first_empty_noncurrent_eden_ages=[b['dead_cycles'] for b in pages[0]['spaces'][0]['blocks'] if b['offset']==0 and b['data'] and not b['current']]))
assert sum(r['observations'] for r in summary)==151
(O/'independent-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
files=[p for p in O.rglob('*') if p.is_file()]+[B/(V+s) for s in ['-runs.json','-driver.exit','-driver.log','.exit']]
proof=dict(natural_boundaries=151,source='Verified c170 full-runtime baseline and production proposal binaries, unchanged workloads.',files={str(p.relative_to(B)):sha(p) for p in files},limitations=['Pagemap residency flags read while the inferior is stopped; no block payload is read or changed.','Debugger and existing census perturb memory/timing. Not peak acceptance.','Offset-zero plus non-current establishes logically free backing, not immediate safe/advisable reclamation or its performance effect.','The existing dead_cycles field at the measured offset40 is captured, not a new age field. Reclamation benefit remains unmeasured.'])
out=B/(V+'-verification.json');out.write_text(json.dumps(proof,indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
    for p in files+[out]:t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(summary))
