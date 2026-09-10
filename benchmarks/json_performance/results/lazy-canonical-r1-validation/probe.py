from pathlib import Path
import hashlib,json,os,re,shutil,subprocess
root=Path('/Users/amlug/projects/perry/json-merged-pr10022')
os.chdir(root)
b=Path('benchmarks/json_performance'); work=b/'.work/lazy-canonical-r1'; dest=work/'canonical-probe'
prov=json.loads((work/'provenance.json').read_text())
worker=work/'worker'; baseline=b/'.work/main-eee/worker'
assert hashlib.sha256(worker.read_bytes()).hexdigest()==prov['files'][str(worker)]
dest.mkdir(exist_ok=False)
old=b/'results/lazy-canonical-probe'
for name in ['whitespace','unicode_escape','slash_escape','duplicate_key','integer_key_order','canonical']:
    shutil.copy2(old/(name+'.json'),dest/(name+'.json'))
extras={
    'escaped_duplicate':r'{"x":1,"\u0078":2}',
    'nested_duplicate':r'{"parent":{"x":1,"x":2}}',
    'index_after_name':r'{"a":0,"2":2,"1":1}',
    'canonical_index_limits':r'{"0":0,"2":2,"4294967294":3,"a":4,"01":5,"4294967295":6}',
    'canonical_escapes':r'{"x":"a\nb\t\"\\/é😀"}',
    'surrogate_and_control':r'{"x":"\ud800","y":"\u0000","z":"\udc00"}',
    'normalized_numbers':r'{"a":1e-400,"b":1.0,"c":-0,"d":9007199254740993,"e":1e400}',
    'canonical_nested':r'{"x":{"x":1},"y":[{},[],{"x":2}]}'
}
for name,record in extras.items():
    (dest/(name+'.json')).write_text('['+','.join([record]*64)+']',encoding='utf-8')
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
stress={'PERRY_GC_SCHEDULE_SEED':'10022','PERRY_GC_SCHEDULE_RATE':'0.1','PERRY_GC_SCHEDULE_ALLOC_KB':'0','PERRY_GC_PROTECT_FROMSPACE':'1'}
matrix=[]
def run(label,exe,args,env):
    with (dest/(label+'.stdout')).open('wb') as out,(dest/(label+'.stderr')).open('wb') as err:
        result=subprocess.run([*map(str,exe),*map(str,args)],env=clean|env,stdout=out,stderr=err,timeout=90)
    assert result.returncode==0,(label,result.returncode)
    output=(dest/(label+'.stdout')).read_bytes()
    fields=list(map(float,re.search(rb'^RESULT (.+)$',output,re.M).group(1).split()))
    verification=[line for line in output.splitlines() if line.startswith(b'VERIFY ')]
    assert len(fields)==7 and len(verification)==1,label
    diagnostic=(dest/(label+'.stderr')).read_text()
    protected=len(re.findall(r'\[gc-fromspace-protect\].*retired_set=#',diagnostic))
    moved=sum(sum(map(int,re.findall(r'\b(?:copied_objects|promoted_objects)=(\d+)',line))) for line in diagnostic.splitlines() if line.startswith('[gc-copy-minor] ran'))
    return verification,fields[5],protected,moved
for fixture in sorted(dest.glob('*.json')):
    name=fixture.stem; args=[fixture,'roundtrip','64','0','verify']
    expected=run(name+'-node',['/opt/homebrew/bin/node',b/'worker.js'],args,{})
    for mode,menv in [('auto',{}),('tape',{'PERRY_JSON_TAPE':'1'}),('direct',{'PERRY_JSON_TAPE':'0'})]:
        for gc,genv in [('normal',{}),('scheduled',stress),('fullgc',{'PERRY_GEN_GC':'0'})]:
            label=name+'-'+mode+'-'+gc
            actual=run(label,[worker],args,menv|genv|{'PERRY_GC_DIAG':'1'})
            assert actual[:2]==expected[:2],label
            if gc=='scheduled': assert actual[2]>0 and actual[3]>0,(label,actual[2:])
            matrix.append(dict(fixture=name,engine='candidate',tape=mode,gc=gc,matches_node=True,protected_retired_sets=actual[2],moved_objects=actual[3]))
    for mode,menv in [('auto',{}),('direct',{'PERRY_JSON_TAPE':'0'})]:
        actual=run(name+'-main-'+mode,[baseline],args,menv)
        matches=actual[:2]==expected[:2]
        if mode=='direct': assert matches,name
        if mode=='auto' and name in ['whitespace','unicode_escape','slash_escape','duplicate_key','integer_key_order']:assert not matches,name
        matrix.append(dict(fixture=name,engine='main',tape=mode,gc='normal',matches_node=matches))
    print('PASS',name,flush=True)
shutil.copy2(work/'provenance.json',dest/'candidate-provenance.json')
shutil.copy2(b/'.work/main-eee/provenance.json',dest/'main-provenance.json')
shutil.copy2(b/'worker.js',dest/'worker.js');shutil.copy2(b/'worker.ts',dest/'worker.ts')
(dest/'matrix.json').write_text(json.dumps(matrix,indent=2)+'\n')
print('PASS',len(matrix),'Perry comparisons',flush=True)
