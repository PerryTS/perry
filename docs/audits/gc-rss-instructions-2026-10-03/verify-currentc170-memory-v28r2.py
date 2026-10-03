"""Independent raw evidence verification. Does not execute measured programs."""
import pathlib, json, hashlib, re, struct, tarfile
B = pathlib.Path('/root/rss-header-20261002')
FILES = {}
def sha(p):
    with pathlib.Path(p).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()
def retain(p):
    p = pathlib.Path(p)
    FILES[str(p.relative_to(B))] = sha(p)
    return p
def load(p): return json.loads(retain(p).read_text())
def check(p, digest): assert sha(retain(p)) == digest, str(p)
def lines(p): return [json.loads(s) for s in retain(p).read_text().splitlines()]
def write(p, x):
    p.write_text(json.dumps(x, indent=2)+'\n'); retain(p)
def receipt(prefix):
    for suffix in ['.exit', '-driver.exit']:
        assert retain(B/(prefix+suffix)).read_text().strip() == '0'
    retain(B/(prefix+'-driver.log'))
oracle = load(B/'gc-currentc170-qb6-v22-noauto-oracle.json')
builds = load(B/'gc-currentc170-qb6-v22-noauto-builds.json')
def product(arm, case, digest):
    row = next(r for r in builds if r['arm']==arm and r['case']==case)
    assert row['binary_sha256'] == digest
    binary = B/'qb6-currentc170-build-v22/bin'/f'gc-qb6-currentc170-v22-noauto-{arm}-{case}'
    assert sha(binary) == digest
    return binary
N = 'gc-currentc170-natural-arena-v26r1'; receipt(N)
natural = load(B/(N+'-runs.json')); assert len(natural)==4
assert {(r['arm'],r['case']) for r in natural} == {(a,c) for a in ['currentc170-base','currentc170-gc'] for c in oracle}
natural_summary = []
for r in natural:
    arm,case = r['arm'],r['case']; product(arm,case,r['binary_sha256'])
    prefix = B/N/f'{N}-{arm}-{case}'
    assert r['rc']==0 and r['reason'] is None
    assert retain(prefix.with_suffix('.app.out')).read_text()==oracle[case]
    retain(prefix.with_suffix('.app.err'))
    check(prefix.with_suffix('.gdb'),r['script_sha256'])
    check(prefix.with_suffix('.observer.py'),r['observer_sha256'])
    rows,bounds = [],[]
    if r['natural_synchronous_fulls']:
        check(prefix.with_suffix('.jsonl'),r['census_sha256'])
        check(prefix.with_suffix('.boundaries.jsonl'),r['boundaries_sha256'])
        rows=lines(prefix.with_suffix('.jsonl'));bounds=lines(prefix.with_suffix('.boundaries.jsonl'))
    assert len(rows)==r['natural_synchronous_fulls'] and len(bounds)==2*len(rows)
    points=[]
    for i,x in enumerate(rows):
        assert x['seq']==i and x['totals']['reachability_pass']
        before,after = bounds[2*i:2*i+2]
        assert before['phase']=='mark_complete_before_arm' and after['phase']=='before_report'
        assert before['full']==after['full']==i+1
        spaces=x['arena']['spaces']; managed=spaces[:5]
        assert sum(s['capacity_bytes'] for s in managed)==x['arena']['capacity_bytes']
        assert sum(s['used_bytes'] for s in managed)==x['arena']['used_bytes']
        for state in ['live','dead','late_marked']:
            for unit in ['bytes','count']:
                key=state+('_bytes' if unit=='bytes' else '_objects')
                assert sum(s[state][unit] for s in spaces)==x['totals'][key]
                assert sum(t[state][unit] for t in x['by_type'])==x['totals'][key]
        rss=int(re.search(r'^Rss:\s+(\d+) kB$',before['smaps_rollup'],re.M)[1])*1024
        points.append(dict(seq=i,pre_arm_rss_bytes=rss,totals=x['totals'],arena=x['arena'],strings=next(t for t in x['by_type'] if t['type']=='string')))
    natural_summary.append(dict(arm=arm,case=case,natural_fulls=len(rows),live_bytes_range=[min(x['totals']['live_bytes'] for x in rows),max(x['totals']['live_bytes'] for x in rows)] if rows else None,highest_pre_arm_rss=sorted(points,key=lambda p:p['pre_arm_rss_bytes'],reverse=True)[:3],first=points[:1],last=points[-1:]))
assert sum(r['natural_synchronous_fulls'] for r in natural)==151
write(B/(N+'-summary.json'),natural_summary)
P='gc-currentc170-peak-mappings-v28r2';receipt(P)
peaks=load(B/(P+'-runs.json'));assert len(peaks)==12
assert {(r['arm'],r['case'],r['repeat']) for r in peaks}=={(a,c,i) for a in ['currentc170-base','currentc170-gc'] for c in oracle for i in range(3)}
for r in peaks:
    binary=product(r['arm'],r['case'],r['binary_sha256'])
    prefix=B/r['origin_prefix']/f"{r['origin_prefix']}-{r['arm']}-{r['case']}-{r['repeat']}"
    for ext,key in [('smaps','smaps_sha256'),('out','stdout_sha256'),('err','stderr_sha256'),('time','time_report_sha256')]: check(prefix.with_suffix('.'+ext),r[key])
    assert prefix.with_suffix('.out').read_text()==oracle[r['case']] and r['rc']==0
    assert int(re.search(r'^RSS_KIB=(\d+)$',prefix.with_suffix('.time').read_text(),re.M)[1])*1024==r['program_max_rss_bytes']
    categories={};total=anon=0;current=None
    for line in prefix.with_suffix('.smaps').read_text().splitlines():
        if re.match(r'^[0-9a-f]+-[0-9a-f]+ ',line):
            fields=line.split(maxsplit=5); current=dict(permissions=fields[1],path=fields[5] if len(fields)>5 else '',values={})
        elif current and ':' in line:
            k,v=line.split(':',1);v=v.strip().split()
            if len(v)==2 and v[1]=='kB': current['values'][k]=int(v[0])*1024
            if k=='Anonymous':
                a=current['values']['Anonymous'];rss=current['values']['Rss'];assert 0<=a<=rss
                key='program executable pages' if current['path']==str(binary) and 'x' in current['permissions'] else 'program other file-backed pages' if current['path']==str(binary) else 'other file-backed pages'
                categories[key]=categories.get(key,0)+rss-a;categories['anonymous pages']=categories.get('anonymous pages',0)+a;total+=rss;anon+=a
    snapshot=load(prefix.with_suffix('.snapshot.json'))
    assert categories==r['categories']==snapshot['categories'] and total==r['sampled_mapping_rss_bytes']==snapshot['rss_bytes'] and anon==r['anonymous_bytes']
    assert r['signed_peak_gap_bytes']==r['program_max_rss_bytes']-total and abs(r['signed_peak_gap_bytes'])<1024*1024
G=B/'primary-arena-geometry-v27/export-r1'
assert retain(G/'complete.exit').read_text().strip()=='0'
geom=load(G/'geometry-verified.json');inputs=load(G/'source-inputs.json');assert len(inputs)==4995
assert sha(G/'source-inputs.json')==geom['inputs_sha256']
for rel,digest in inputs.items(): assert sha(G.parent/'source'/rel)==digest,rel
for rel,digest in geom['raw_sha256'].items(): check(G/'raw'/rel,digest)
for name in ['vec','box_slice']:
    raw=G/'raw'/('vec.bin' if name=='vec' else 'box.bin'); words=struct.unpack('<'+'Q'*(len(raw.read_bytes())//8),raw.read_bytes())
    for field,offset in geom['geometry'][name]['word_offsets'].items(): assert words[offset//8]==geom['geometry'][name][field]
for name in ['commands.json','excluded-owned-generated-inputs.json','geometry.log','build.log']:retain(G/name)
L='gc-private-currentc170-v25-linked';receipt(L)
linked=load(B/(L+'-runs.json')); assert len(linked)==66
products=load(B/(L+'-products.json'))
for path,digest in products.items():assert sha(path)==digest
retain(B/(L+'-derived-source.json'))
moving=0
for r in linked:
    assert r['rc']==0 and r['reason'] is None
    binary=pathlib.Path(r['cmd'][-1] if r['mode']=='build' else r['cmd'][0]);assert sha(binary)==r['binary_sha256']
    if r['mode']=='build':
        assert sha(r['cmd'][2])==r['source_sha256'];continue
    expected=retain(B/'logs'/f"{L}-node-{r['case']}.out").read_text()
    out=retain(B/'logs'/(r['label']+'.out'));err=retain(B/'logs'/(r['label']+'.err'))
    assert out.read_text()==r['stdout']==expected
    if r['mode']=='moving':
        raw=err.read_text().splitlines();cycles=[json.loads(s) for s in raw if s.startswith('{')]
        copied=sum(c.get('copying_nursery',{}).get('copied_objects',0) for c in cycles)
        assert copied==r['copied_objects'] and copied>0
        assert any(s.startswith('[gc-fromspace-protect]') and 'retired_set=' in s for s in raw);moving+=1
assert moving==22
failures={}
for prefix in ['gc-currentc170-natural-arena-v26','gc-currentc170-peak-mappings-v28','gc-currentc170-peak-mappings-v28r1','primary-arena-geometry-v27']:
    for suffix in ['-driver.exit','-driver.log']:
        p=B/(prefix+suffix)
        if p.exists():retain(p);failures[p.name]=p.read_text()[:1000]
proof=dict(natural_full_censuses=151,near_peak_mappings=12,linked_executions=44,actually_moving_and_protected=22,geometry_tracked_inputs=4995,files=FILES,limitations=['Natural census/GDB observations perturb memory and timing and are not peak acceptance.','smaps reads are non-atomic; all signed distances from GNU time max RSS retained.','Geometry is a test build; shipping access offsets have not yet been independently checked.','This does not establish a minimum RSS or a reclaimable empty-block budget.'],preserved_initial_failures=failures)
out=B/'currentc170-memory-observation-v28r2-verification.json'
out.write_text(json.dumps(proof,indent=2)+'\n')
with tarfile.open(B/'currentc170-memory-observation-v28r2-evidence.tar.gz','w:gz') as t:
    for rel in FILES:t.add(B/rel,arcname=rel)
    t.add(out,arcname=out.name)
print(json.dumps({k:v for k,v in proof.items() if k not in ['files','preserved_initial_failures']}))
