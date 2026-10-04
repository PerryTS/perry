import pathlib,json,hashlib,statistics,tarfile
B=pathlib.Path('/root/rss-header-20261002');V='gc-lazy-instruction-repeat-main07-v67'
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert all((B/(V+n)).read_text().strip()=='0' for n in ['.exit','-driver.exit'])
i=json.loads((B/(V+'-inputs.json')).read_text());rows=json.loads((B/(V+'-runs.json')).read_text());assert len(rows)==36
for n in ['verification','runs','builds']:assert sha(B/(i['prior']+'-'+n+'.json'))==i['prior_'+n+'_sha256']
summary={};files=set()
for case,c in i['cases'].items():
    assert sha(c['source'])==c['source_sha256']
    assert (B/'logs'/(V+'-oracle-'+case+'.out')).read_text()==c['oracle']
    summary[case]={}
    for arm,binary in c['binaries'].items():
        assert sha(binary['path'])==binary['sha256']
        selected=[r for r in rows if r['case']==case and r['arm']==arm]
        assert len(selected)==9 and {r['repeat'] for r in selected}==set(range(9))
        for r in selected:
            assert r['rc']==0 and r['reason'] is None and r['correct'] and r['binary_sha256']==binary['sha256']
            assert r['cmd']==['taskset','-c','2','perf','stat','-x,','-e','instructions,cycles,minor-faults,major-faults',binary['path'],*c['args']]
            assert r['env']=={'MIMALLOC_ALLOW_THP':'0'}
            assert (B/'logs'/(r['label']+'.out')).read_text()==r['stdout']==c['oracle']
            err=(B/'logs'/(r['label']+'.err')).read_text()
            events={f[2]:int(f[0]) for line in err.splitlines() if len(f:=line.split(','))>2 and f[2] in ['instructions','cycles','minor-faults','major-faults']}
            assert events==r['events'] and len(events)==4
        summary[case][arm]={n:dict(values=(v:=[r['events'][n] for r in selected]),min=min(v),median=statistics.median(v),max=max(v)) for n in selected[0]['events']}
    summary[case]['instruction_percent_vs_gc']=100*(summary[case]['lazy']['instructions']['median']/summary[case]['gc']['instructions']['median']-1)
proof=dict(inputs=i,runs=36,repeats_per_case_arm=9,summary=summary,scope='Fresh exact-binary instruction/fault repeats; original full17 retained. No RSS claim from perf-only repeats.')
(B/(V+'-verification.json')).write_text(json.dumps(proof,indent=2)+'\n')
files.update(B/(V+n) for n in ['-inputs.json','-runs.json','-verification.json','.exit','-driver.exit','-driver.log'])
files.update([pathlib.Path(__file__),B/'primary-lazy-instruction-repeat-main07-v67.py'])
for label in [*[V+'-oracle-'+c for c in i['cases']],*[r['label'] for r in rows]]:files.update(B/'logs'/(label+'.'+ext) for ext in ['out','err'])
m=B/(V+'-evidence-files.json');m.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
    for p in sorted(files|{m}):t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(proof))
