"""Latest-release ScriptC source, binary, raw output and peak-RSS verification."""
import pathlib,json,hashlib,re,statistics,tarfile
B=pathlib.Path('/root/rss-header-20261002');V='comparators-scriptc022-v38';T=B/'comparetools-scriptc022-v38'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert (B/(V+'.exit')).read_text().strip()=='0' and (B/(V+'-driver.exit')).read_text().strip()=='0'
version=json.loads((B/(V+'-version.json')).read_text());assert version['version']=='0.2.2' and sha(T/'node_modules/.bin/scriptc')==version['compiler_sha256'] and sha(T/'package-lock.json')==version['lock_sha256']
lock=json.loads((T/'package-lock.json').read_text())
assert lock['packages']['node_modules/scriptc']['version']=='0.2.2'
assert lock['packages']['node_modules/scriptc']['integrity']=='sha512-DWSTPtyNYQxfrBeip9uvcgdnDRYBcJzwcVMmrFZsYO0sXxzqaA3KLuik543YIuYyatAuMO4BnYRCOgI2ny3g1Q=='
rows=json.loads((B/(V+'-runs.json')).read_text());oracle=json.loads((B/(V+'-oracle.json')).read_text())
cases=['00-noop','15-crc32','23-binary-trees','30-string-build','31-json','51-pipeline','60-ring-churn'];assert set(oracle)==set(cases)
summary=[]
for case in cases:
 builds=[x for x in rows if x['case']==case and x['mode']=='build'];assert len(builds)==1
 build=builds[0];assert sha(build['cmd'][2])==build['source_sha256'] and sha(build['cmd'][0])==build['compiler_sha256']==version['compiler_sha256']
 runs=[x for x in rows if x['case']==case and x['mode']=='plain']
 if not build['compiled']:
  assert build['rc']!=0 or build['reason'] is not None;assert not runs
  summary.append(dict(case=case,engine='scriptc',version='0.2.2',correct=False,status='compile_failed',rc=build['rc'],reason=build['reason']));continue
 assert sha(build['cmd'][-1])==build['binary_sha256'] and len(runs)==3 and {x['repeat'] for x in runs}=={0,1,2}
 for x in runs:
  assert sha(x['cmd'][-1])==x['binary_sha256']==build['binary_sha256']
  assert (B/'logs'/(x['label']+'.out')).read_text()==x['stdout']
  err=(B/'logs'/(x['label']+'.err')).read_text()
  assert int(re.search(r'RSS_KIB=(\d+)',err)[1])*1024==x['peak_rss_bytes']
  assert x['correct']==(x['rc']==0 and x['reason'] is None and x['stdout']==oracle[case])
 correct=all(x['correct'] for x in runs)
 summary.append(dict(case=case,engine='scriptc',version='0.2.2',correct=correct,status='correct' if correct else 'output_or_execution_failed',rss_mib=statistics.median(x['peak_rss_bytes']/2**20 for x in runs) if correct else None))
for x in rows:
 for ext in ['out','err']:assert (B/'logs'/(x['label']+'.'+ext)).exists()
 assert (B/'logs'/(x['label']+'.out')).read_text()==x['stdout']
proof=dict(version=version,complete_build_cases=7,summary=summary,scope='Seven unchanged standalone sources; compile/execution failures retained without winning RSS. Real TypeScript/Zod cross-engine equivalence is not established.',inputs={n:sha(B/(V+'-'+n+'.json')) for n in ['runs','oracle','version']})
(B/(V+'-verification.json')).write_text(json.dumps(proof,indent=2)+'\n')
files=set(B/(V+'-'+n+'.json') for n in ['runs','oracle','version','verification']);files.update([B/(V+'.exit'),B/(V+'-driver.exit'),B/(V+'-driver.log'),T/'install.log',T/'manual-native-install.log',T/'package-lock.json',pathlib.Path(__file__),B/'primary-scriptc022-v38.py'])
for row in rows:
 for ext in ['out','err']:files.add(B/'logs'/(row['label']+'.'+ext))
for c in cases:
 for ext in ['out','err']:files.add(B/'logs'/(V+'-oracle-'+c+'.'+ext))
m=B/(V+'-evidence-files.json');m.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
 for p in sorted(files|{m}):t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(proof))
