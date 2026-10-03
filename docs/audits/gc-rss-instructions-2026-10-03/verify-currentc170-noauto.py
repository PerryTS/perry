import pathlib,json,hashlib,importlib.util,re
B=pathlib.Path(__file__).resolve().parent;V='gc-currentc170-qb6-v22-noauto';Q=B/'qb6-currentc170-build-v22'
def load(n):return json.loads((B/(V+'-'+n+'.json')).read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
spec=importlib.util.spec_from_file_location('summary',B/'summarize-gc-comparison.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
r=load('runs');b=load('builds');o=load('oracle');a=['currentc170-base','currentc170-gc'];m.validate(r,o,a,('tscwork','zodwork'),True);m.validate_builds(r,b)
for row in b:
 assert sha(Q/'sources/real'/(row['case']+'.ts'))==row['source_sha256']
 assert sha(Q/'bin'/f'gc-qb6-currentc170-v22-noauto-{row["arm"]}-{row["case"]}')==row['binary_sha256']
for row in r:
 assert (B/'logs'/(row['label']+'.out')).read_text()==row['stdout']
 err=(B/'logs'/(row['label']+'.err')).read_text()
 if row['mode']=='plain':assert int(re.search(r'RSS_KIB=(\d+)',err)[1])*1024==row['peak_rss_bytes']
 else:
  events={f[2]:int(f[0]) for l in err.splitlines() if len(f:=l.split(','))>2 and f[2] in ['instructions','cycles','minor-faults','major-faults']};assert events==row['events']
s=load('summary');assert s['cases']==m.summarize(r,a,('tscwork','zodwork'),True)
x=dict(complete_cells=24,source_binary_stdout_rss_counter_and_summary_verified=True,inputs={n:sha(B/(V+'-'+n+'.json')) for n in ['runs','builds','oracle','summary']},build_proof_sha256=sha(B/'qb6-currentc170-build-v22-verification.json'),limitations='Three repetitions on shared original host. Signed costs retained; passing correctness does not imply every cost improves.')
(B/(V+'-verification.json')).write_text(json.dumps(x,indent=2)+'\n');print(x)
