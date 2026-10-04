import pathlib, json, hashlib, re, statistics, tarfile
B = pathlib.Path('/root/rss-header-20261002')
V = 'gc-retain-release-main07-v58'
def sha(p):
    with pathlib.Path(p).open('rb') as f:
        return hashlib.file_digest(f,'sha256').hexdigest()
assert all((B/(V+n)).read_text().strip()=='0' for n in ['.exit','-driver.exit'])
inputs=json.loads((B/(V+'-inputs.json')).read_text())
prior=inputs['prior']
assert sha(B/(prior+'-verification.json'))==inputs['prior_verification_sha256']
assert sha(B/(prior+'-runs.json'))==inputs['prior_runs_sha256']
assert sha(B/(prior+'-builds.json'))==inputs['prior_builds_sha256']
assert sha(inputs['source'])==inputs['source_sha256']
assert (B/'logs'/(V+'-oracle.out')).read_text()==inputs['oracle']
rows=json.loads((B/(V+'-runs.json')).read_text())
assert len(rows)==27
summary={}
for arm,binary in inputs['binaries'].items():
    assert sha(binary['path'])==binary['sha256']
    selected=[r for r in rows if r['arm']==arm]
    assert len(selected)==9 and {r['repeat'] for r in selected}==set(range(9))
    for r in selected:
        assert r['rc']==0 and r['reason'] is None and r['correct']
        assert r['cmd']==['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',binary['path']]
        assert r['binary_sha256']==binary['sha256'] and r['env']=={'MIMALLOC_ALLOW_THP':'0'}
        assert (B/'logs'/(r['label']+'.out')).read_text()==r['stdout']==inputs['oracle']
        assert int(re.search(r'RSS_KIB=(\d+)',(B/'logs'/(r['label']+'.err')).read_text())[1])*1024==r['peak_rss_bytes']
    values=[r['peak_rss_bytes'] for r in selected]
    summary[arm]=dict(values=values,min=min(values),median=statistics.median(values),max=max(values))
changes={a:100*(summary[a]['median']/summary['gc']['median']-1) for a in ['entry','once']}
proof=dict(inputs=inputs,runs=27,repeats_per_arm=9,summary=summary,median_rss_percent_vs_gc=changes,
    scope='Independent fresh RSS repeat proof; original V46 measurements remain preserved, no instruction claim.')
(B/(V+'-verification.json')).write_text(json.dumps(proof,indent=2)+'\n')
files={B/(V+n) for n in ['-inputs.json','-runs.json','-verification.json','.exit','-driver.exit','-driver.log']}
files.update([pathlib.Path(__file__),B/'primary-retain-release-main07-v58.py'])
for label in [V+'-oracle',*[r['label'] for r in rows]]:
    files.update(B/'logs'/(label+'.'+ext) for ext in ['out','err'])
m=B/(V+'-evidence-files.json');m.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
    for p in sorted(files|{m}):t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(proof,indent=2))
