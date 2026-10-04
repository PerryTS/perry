"""Source-bound seven-case RSS join of accepted Perry main07 and latest compilers."""
import pathlib,json,hashlib,re,statistics,subprocess,tarfile
B=pathlib.Path('/root/rss-header-20261002');V='comparators-once-main07-v66';P='gc-auto-entry-once-main07-v46';C='comparators-20261003';SC='comparators-scriptc022-v38';T=B/'comparetools-20261003'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def read(name):return json.loads((B/name).read_text())
versions=read('comparators-20261004-v48-version-check.json');old_versions=read(C+'-versions.json');scriptc=read(SC+'-verification.json');perry=read(P+'-verification.json')
assert versions['scriptc']['version']=='0.2.2'==scriptc['version']['version']
assert versions['porffor']['version']==old_versions['porffor']['npm_latest'] and versions['porffor']['remote_commit']==old_versions['porffor']['remote_commit']
assert versions['hermes']['remote_commit']==old_versions['hermes']['remote_commit']
lock=json.loads((T/'package-lock.json').read_text())['packages']['node_modules/porffor'];assert lock['version']==versions['porffor']['version'] and lock['integrity']==versions['porffor']['integrity']
assert subprocess.check_output(['git','-C',str(T/'hermes'),'rev-parse','HEAD'],text=True).strip()==versions['hermes']['remote_commit']
subprocess.run(['git','-C',str(T/'hermes'),'diff','--quiet'],check=True)
for engine,exe in [('porffor',T/'node_modules/.bin/porf'),('shermes',T/'hermes-build/bin/shermes')]:assert sha(exe)==old_versions['executables'][engine]['sha256']
for name,h in scriptc['inputs'].items():assert sha(B/(SC+'-'+name+'.json'))==h
for name,h in perry['inputs'].items():assert sha(B/(P+'-'+name+'.json'))==h
p_rows=read(P+'-runs.json');p_builds=read(P+'-builds.json');c_rows=read(C+'-runs.json');sc_rows=read(SC+'-runs.json')
p_oracle=read(P+'-oracle.json');c_oracle=read(C+'-oracle.json');sc_oracle=read(SC+'-oracle.json')
cases=['00-noop','15-crc32','23-binary-trees','30-string-build','31-json','51-pipeline','60-ring-churn'];rows=[];files=set();source_hashes={}
assert perry['base']=='07e50b14a3d23e8f914193ec4d5a679adbd7c37a' and perry['complete_cells']==306
for case in cases:
 source=B/('extra-sources' if case in ['00-noop','15-crc32'] else 'sources')/(case+'.ts');source_hashes[case]=sha(source)
 assert p_oracle[case]==c_oracle[case]==sc_oracle[case]
 row=dict(case=case,source_sha256=source_hashes[case],engines={})
 for arm in ['gc','once']:
  build=next(x for x in p_builds if x['arm']==arm and x['source']==str(source));assert build['rc']==0 and build['reason'] is None and build['source_sha256']==source_hashes[case]
  selected=[x for x in p_rows if x['case']==case and x['arm']==arm and x['mode']=='plain'];assert len(selected)==3 and {x['repeat'] for x in selected}=={0,1,2}
  rss=[]
  for run in selected:
   assert run['rc']==0 and run['reason'] is None and run['correct'] and run['stdout']==p_oracle[case]
   assert sha(run['cmd'][3])==run['binary_sha256']==build['binary_sha256']
   raw=B/'logs'/(run['label']+'.err');assert int(re.search(r'RSS_KIB=(\d+)',raw.read_text())[1])*1024==run['peak_rss_bytes']
   assert (B/'logs'/(run['label']+'.out')).read_text()==run['stdout'];rss.append(run['peak_rss_bytes']/2**20)
   files.update([raw,B/'logs'/(run['label']+'.out')])
  row['engines']['perry_'+arm]=dict(correct=True,rss_mib=statistics.median(rss),rss_mib_values=rss,build_prefix=P)
 for engine in ['porffor','shermes','scriptc']:
  all_rows=sc_rows if engine=='scriptc' else c_rows
  build=next(x for x in all_rows if x['case']==case and x['mode']=='build' and (engine=='scriptc' or x['engine']==engine))
  assert build['source_sha256']==source_hashes[case]
  if engine=='shermes':
   js=pathlib.Path(build['cmd'][2]);assert sha(js)==build['js_sha256']
   code="const fs=require('fs');const m=require('node:module');process.stdout.write(m.stripTypeScriptTypes(fs.readFileSync(process.argv[1],'utf8')));"
   stripped=subprocess.check_output(['/opt/node-v26.5.1-linux-x64/bin/node','-e',code,str(source)],text=True,stderr=subprocess.DEVNULL);assert stripped==js.read_text()
  if not build['compiled']:
   assert engine=='scriptc' and case=='31-json' and build['rc']!=0
   row['engines'][engine]=dict(correct=False,status='compile_failed',rc=build['rc'],rss_mib=None)
   files.update([B/'logs'/(build['label']+'.'+ext) for ext in ['out','err']]);continue
  assert build['rc']==0 and build['reason'] is None and sha(build['cmd'][-1])==build['binary_sha256']
  selected=[x for x in all_rows if x['case']==case and x['mode']=='plain' and (engine=='scriptc' or x['engine']==engine)];assert len(selected)==3 and {x['repeat'] for x in selected}=={0,1,2}
  rss=[]
  for run in selected:
   assert run['rc']==0 and run['reason'] is None and run['correct'] and run['stdout']==p_oracle[case]
   assert run['binary_sha256']==build['binary_sha256'] and sha(run['cmd'][3])==run['binary_sha256']
   raw=B/'logs'/(run['label']+'.err');assert int(re.search(r'RSS_KIB=(\d+)',raw.read_text())[1])*1024==run['peak_rss_bytes']
   assert (B/'logs'/(run['label']+'.out')).read_text()==run['stdout'];rss.append(run['peak_rss_bytes']/2**20)
   files.update([raw,B/'logs'/(run['label']+'.out')])
  row['engines'][engine]=dict(correct=True,rss_mib=statistics.median(rss),rss_mib_values=rss,build_prefix=SC if engine=='scriptc' else C)
 rows.append(row)
proof=dict(perry_base=perry['base'],perry_runtime=perry['private_once'],draft_source_equivalent_commit='4401b3b981',latest_version_check=versions,rows=rows,scope='Seven unchanged standalone workloads on the retained primary measurement host. Measurements were taken at different times, with three plain RSS repetitions each. Perry once is the adopted draft candidate, source-equivalent to 8e3013733d on main07; Perry gc is its preceding GC control. All engine measurements retain their original timestamps. ScriptC JSON compile failure has no RSS rank; real TS/Zod cross-engine equivalence remains unproven.',inputs={n:sha(B/n) for n in [P+'-verification.json',P+'-runs.json',P+'-builds.json',C+'-versions.json',C+'-runs.json',C+'-oracle.json',SC+'-verification.json',SC+'-runs.json','comparators-20261004-v48-version-check.json']})
(B/(V+'-join.json')).write_text(json.dumps(proof,indent=2)+'\n')
files.update([B/n for n in proof['inputs']]);files.update([B/(V+'-join.json'),pathlib.Path(__file__)])
m=B/(V+'-evidence-files.json');m.write_text(json.dumps({str(x.relative_to(B)):sha(x) for x in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
 for x in sorted(files|{m}):t.add(x,arcname=str(x.relative_to(B)))
print(json.dumps(proof))
