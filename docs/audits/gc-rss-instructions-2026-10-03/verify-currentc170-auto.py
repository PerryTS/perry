import pathlib,json,hashlib,importlib.util,re,subprocess
B=pathlib.Path(__file__).resolve().parent;V='gc-auto-currentc170-qb6-v22';Q=B/'qb6-currentc170-build-v22'
def load(n):return json.loads((B/(V+'-'+n+'.json')).read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
spec=importlib.util.spec_from_file_location('summary',B/'summarize-gc-comparison.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
r=load('runs');b=load('builds');o=load('oracle');a=['currentc170-base','currentc170-gc'];m.validate(r,o,a,m.AUTO_CASES,True);m.validate_builds(r,b);assert len(b)==30
for row in b:
 stem=pathlib.Path(row['source']).stem
 if stem in ['00-noop','15-crc32']:source=Q/'extra-sources'/(stem+'.ts')
 elif stem in ['tscwork','zodwork']:source=Q/'sources/real'/(stem+'.ts')
 else:source=Q/'sources'/(stem+'.ts')
 assert sha(source)==row['source_sha256']
 binary=Q/'bin'/f'gc-auto-qb6-currentc170-v22-{row["arm"]}-{stem}';symbol=pathlib.Path(str(binary)+'-symbols');assert sha(binary)==row['binary_sha256'] and sha(symbol)==row['symbol_sha256']
for row in r:
 assert (B/'logs'/(row['label']+'.out')).read_text()==row['stdout']
 err=(B/'logs'/(row['label']+'.err')).read_text()
 if row['mode']=='plain':assert int(re.search(r'RSS_KIB=(\d+)',err)[1])*1024==row['peak_rss_bytes']
 else:
  events={f[2]:int(f[0]) for l in err.splitlines() if len(f:=l.split(','))>2 and f[2] in ['instructions','cycles','minor-faults','major-faults']};assert events==row['events']
s=load('summary');assert s['cases']==m.summarize(r,a,m.AUTO_CASES,True)
x=dict(complete_cells=204,measured_cases=17,excluded_cases=[],source_binary_stdout_rss_counter_and_summary_verified=True,inputs={n:sha(B/(V+'-'+n+'.json')) for n in ['runs','builds','oracle','summary']},build_proof_sha256=sha(B/'qb6-currentc170-build-v22-verification.json'),limitations='Three repetitions on shared original host. Signed costs retained; passing correctness does not imply every cost improves.')
(B/(V+'-verification.json')).write_text(json.dumps(x,indent=2)+'\n');print(x)
