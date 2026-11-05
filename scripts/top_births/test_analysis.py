import csv,gzip,json,os,subprocess,tempfile
from pathlib import Path
R=Path(tempfile.mkdtemp(prefix='cxt1-graph-',dir=None))
rs=[]
for n in [100,101,102,200,300,301,302]:rs.append(dict(record='node',id=n,top=True,tokens=[0],label='fixture',topRoot=n,topOrigin='fixture'))
for n in [100,200,300]:rs.append(dict(record='birth-injection',node=n,injector='fixture.rs',line=n,why='native result',source=2**32-1,call=2**32-1,escape=2**32-1,owner=2**32-1))
rs.append(dict(record='birth-injection',node=300,injector='second-rule.rs',line=99,why='native result',source=2**32-1,call=2**32-1,escape=2**32-1,owner=2**32-1))
for a,b in [(100,101),(200,101),(101,102),(300,301),(301,302),(302,301)]:rs.append(dict(record='copy-edges',source=a,targets=[b]))
for n,start in [(102,1),(102,1)]+[(301,s) for s in [5,9,13,17,21,25,29,33]]:rs.append(dict(record='site',file='fixture.ts',start=start,end=start+2,kind='property',key='x',receiver=n,result=999,callee=None,index=None,call=None))
next(r for r in rs if r['record']=='site' and r['start']==9)['end']=0
(R/'base.dump').write_text(''.join('D '+json.dumps(r)+'\n' for r in rs))
with gzip.open(R/'base.ledger.tsv.gz','wt') as f:
 w=csv.DictWriter(f,fieldnames=['file','start','end','kind','key','verdict'],delimiter='\t');w.writeheader()
 for s in [1,5,9,13,17,21,25,29,33]:w.writerow(dict(file='fixture.ts',start=s,end=s+2,kind='property',key='x',verdict='unproven'))
env=dict(os.environ,PERRY_BIRTH_OUTPUT=str(R))
subprocess.run(['python3',str(Path(__file__).with_name('an.py')),'base'],env=env,stdout=subprocess.PIPE,check=True)
rows=list(csv.DictReader(open(R/'raw-births.tsv'),delimiter='\t'))
assert {(int(r['node']),int(r['sole']),int(r['total'])) for r in rows}=={(100,0,1),(200,0,1),(300,8,8)},rows
print('PASS: two origins share one identity; sole origin through SCC; duplicate views and multiple injectors at one birth node deduplicated')

# an.py must tolerate the chain/opener records of a census dump: birth-arrival is ignored, an unknown
# birth-* kind is skipped with one warning on stderr (red on base: KeyError at an.py:47).
rs2=rs+[dict(record='birth-arrival',node=100,seq=5,source=2**32-1,cause=2**32-1,call=2**32-1,parent=2**32-1,injected=True,gate=None),
 dict(record='birth-opener',holder=1,key=None),dict(record='birth-zzz',x=1),dict(record='birth-zzz',x=2)]
(R/'base.dump').write_text(''.join('D '+json.dumps(r)+'\n' for r in rs2))
(R/'base.idx.pkl').unlink(missing_ok=True)
pr=subprocess.run(['python3',str(Path(__file__).with_name('an.py')),'base'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
assert pr.returncode==0,pr.stderr
assert pr.stderr.count('skipping unknown record kind birth-zzz')==1 and 'arrival' not in pr.stderr,pr.stderr
rows2=list(csv.DictReader(open(R/'raw-births.tsv'),delimiter='\t'))
assert {(int(r['node']),int(r['sole']),int(r['total'])) for r in rows2}=={(100,0,1),(200,0,1),(300,8,8)},rows2
print('PASS: birth-arrival ignored, unknown birth-* kind skipped with one warning')
