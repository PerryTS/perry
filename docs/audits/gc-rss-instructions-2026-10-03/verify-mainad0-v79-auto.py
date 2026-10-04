import pathlib,json,hashlib,re,importlib.util,subprocess,tarfile
B=pathlib.Path('/root/rss-header-20261002');R=B/'primary-main07-v39';E=R/'export';V='gc-auto-mainad0-v79'
exports={'base':B/'primary-mainad0-base-v77/export','gc':B/'primary-mainad0-once-v74/export'}
trees={'base':B/'primary-mainad0-base-v77/source','gc':B/'primary-mainad0-once-v74/source'}
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def load(n):return json.loads((B/(V+'-'+n+'.json')).read_text())
assert (B/(V+'.exit')).read_text().strip()=='0' and (B/(V+'-driver.exit')).read_text().strip()=='0'
r,b,o,s,products=[load(n) for n in ['runs','builds','oracle','summary','products']]
assert len(r)==204 and len(b)==30 and len(o)==17
assert all(sha(p)==h for p,h in products.items())
for arm in ['base','gc']:
 inputs=json.loads((exports[arm]/'source-inputs.json').read_text());assert len(inputs)=={'base':5006,'gc':5014}[arm]
 assert all(sha(trees[arm]/n)==h for n,h in inputs.items())
for row in b:
 assert row['rc']==0 and row['reason'] is None and sha(row['source'])==row['source_sha256']
 assert sha(row['cmd'][0])==row['compiler_sha256']==products[str(exports[row['arm']]/'perry')]
 assert row['env']['PERRY_RUNTIME_DIR']==str(exports[row['arm']]) and row['env']['PERRY_WORKSPACE_ROOT']==str(trees[row['arm']])
 symbol=pathlib.Path(row['cmd'][-1]);assert symbol.name.endswith('-symbols')
 plain=pathlib.Path(str(symbol).removesuffix('-symbols'))
 assert sha(plain)==row['binary_sha256'] and sha(symbol)==row['symbol_sha256']
 for file in [plain,symbol]:
  text=B/(V+'-independent-text.tmp')
  subprocess.run(['objcopy','--dump-section',f'.text={text}',str(file),'/dev/null'],check=True)
  assert sha(text)==row['text_sha256'];text.unlink()
 assert (B/'logs'/(row['label']+'.out')).read_text()==row['stdout']
for row in r:
 assert (B/'logs'/(row['label']+'.out')).read_text()==row['stdout']==o[row['case']]
 err=(B/'logs'/(row['label']+'.err')).read_text()
 if row['mode']=='plain':assert int(re.search(r'RSS_KIB=(\d+)',err)[1])*1024==row['peak_rss_bytes']
 else:
  events={f[2]:int(f[0]) for line in err.splitlines() if len(f:=line.split(','))>2 and f[2] in ['instructions','cycles','minor-faults','major-faults']};assert events==row['events']
spec=importlib.util.spec_from_file_location('summary',B/'summarize-gc-comparison.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
for name,pair in [('gc_vs_base',['base','gc'])]:
 rows=[x for x in r if x['arm'] in pair];m.validate(rows,o,pair,m.AUTO_CASES,True);assert s['comparisons'][name]==m.summarize(rows,pair,m.AUTO_CASES,True)
m.validate_builds(r,b)
assert s['inputs']=={n:sha(B/(V+'-'+n+'.json')) for n in ['runs','builds','oracle','products']}
auto=load('auto-runtime-products');assert auto
for p,x in auto.items():
 assert pathlib.Path(p).read_text()==x['stamp'] and x['archives']
 assert all(sha(n)==h for n,h in x['archives'].items())
proof=dict(base='ad0a2617bf0b9708032cf89c86575e9bd69fb074',production_runtime='0aa483aa91ab901583002ec0174a50884b60ad81',complete_cells=204,builds=30,all_builds_fresh=True,cases=17,all_raw_sources_products_oracles_counters_verified=True,auto_runtime_stamps=len(auto),inputs={n:sha(B/(V+'-'+n+'.json')) for n in ['runs','builds','oracle','products','summary','auto-runtime-products']})
(B/(V+'-verification.json')).write_text(json.dumps(proof,indent=2)+'\n')
files=set(B/(V+'-'+n+'.json') for n in ['runs','builds','oracle','products','summary','auto-runtime-products','verification','completion']);files.update([B/(V+'.exit'),B/(V+'-driver.exit'),B/(V+'-driver.log'),pathlib.Path(__file__),B/'primary-mainad0-v79-auto.py'])
for rows in [r,b]:
 for row in rows:
  for ext in ['out','err']:files.add(B/'logs'/(row['label']+'.'+ext))
for c in o:
 for ext in ['out','err']:files.add(B/'logs'/(V+'-oracle-'+c+'.'+ext))
manifest=B/(V+'-evidence-files.json');manifest.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
 for p in sorted(files|{manifest}):t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(proof,indent=2))
