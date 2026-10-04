"""Source/product/output/telemetry binding for the diagnostic ring run."""
import pathlib,json,hashlib,re,statistics,tarfile
B=pathlib.Path('/root/rss-header-20261002');V='gc-main07-ring-v50';R=B/'primary-main07-v39';C=B/'primary-entry-once-main07-v45'
exports={'gc':R/'export/gc','entry':R/'export/entry','once':C/'export'};trees={'gc':R/'source-gc','entry':R/'source-entry','once':C/'source'}
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def load(n):return json.loads((B/(V+'-'+n+'.json')).read_text())
assert (B/(V+'.exit')).read_text().strip()=='0' and (B/(V+'-driver.exit')).read_text().strip()=='0'
rows=load('runs');products=load('products');oracle=load('oracle');scope=load('scope')
assert len(rows)==9 and len(products)==9 and scope['base']=='07e50b14a3d23e8f914193ec4d5a679adbd7c37a'
assert oracle['rc']==0 and oracle['reason'] is None
source=B/'sources/60-ring-churn.ts';assert sha(source)==scope['source_sha256']
files=set();summary={}
for arm in exports:
 path=C/'export/source-inputs.json' if arm=='once' else R/'export'/(arm+'-source-inputs.json')
 inputs=json.loads(path.read_text());assert len(inputs)==(5011 if arm=='once' else 5010) and all(sha(trees[arm]/n)==h for n,h in inputs.items())
 selected=[r for r in rows if r['arm']==arm];assert len(selected)==3 and {r['mode'] for r in selected}=={'build','plain','trace'}
 build=next(r for r in selected if r['mode']=='build');binary=B/'bin'/(V+'-'+arm)
 assert build['cmd']==[str(exports[arm]/'perry'),'compile',str(source),'--no-auto-optimize','-o',str(binary)]
 assert build['env']['PERRY_RUNTIME_DIR']==str(exports[arm]) and build['env']['PERRY_WORKSPACE_ROOT']==str(trees[arm]) and build['env']['PERRY_NO_CACHE']=='1'
 assert sha(binary)==build['binary_sha256'] and build['source_sha256']==sha(source) and build['source_manifest_sha256']==sha(path)
 for n in ['perry','libperry_runtime.a','libperry_stdlib.a']:assert products[str(exports[arm]/n)]==sha(exports[arm]/n)
 rss={}
 for row in selected:
  assert row['rc']==0 and row['reason'] is None
  assert (B/'logs'/(row['label']+'.out')).read_text()==row['stdout']
  files.update([B/'logs'/(row['label']+'.'+ext) for ext in ['out','err']])
  if row['mode']=='build':continue
  assert row['correct'] and row['stdout']==oracle['stdout'] and row['binary_sha256']==sha(binary)
  assert row['cmd']==['/usr/bin/time','-f','RSS_KIB=%M WALL=%e USER=%U SYS=%S',str(binary)]
  gcenv={k:v for k,v in row['env'].items() if k.startswith('PERRY_GC_')}
  assert gcenv==({} if row['mode']=='plain' else {'PERRY_GC_TRACE':'1','PERRY_GC_DIAG':'1'})
  err=(B/'logs'/(row['label']+'.err')).read_text();assert int(re.search(r'RSS_KIB=(\d+)',err)[1])*1024==row['peak_rss_bytes'];rss[row['mode']]=row['peak_rss_bytes']
 trace=next(r for r in selected if r['mode']=='trace')
 cycles=[json.loads(line) for line in (B/'logs'/(trace['label']+'.err')).read_text().splitlines() if line.startswith('{')]
 assert cycles==load(arm+'-cycles') and cycles
 for c in cycles:
  for when in ['before','after']:
   a=c['arena_bytes'][when]
   for leaf,total in [('reserved_bytes','total_reserved_bytes'),('in_use_bytes','total_in_use_bytes'),('block_count','total_block_count')]:
    assert sum(a[space][leaf] for space in ['arena','survivor0','survivor1','longlived','old'])==a[total]
 def distribution(xs):return dict(min=min(xs),median=statistics.median(xs),max=max(xs))
 summary[arm]=dict(cycles=len(cycles),collection_kinds=sorted({c['collection_kind'] for c in cycles}),rss_bytes=rss,copied_objects=sum(c['copying_nursery']['copied_objects'] for c in cycles),eden_live_bytes=distribution([c['copying_nursery']['eden_live_bytes'] for c in cycles]),arena={when:{space:{leaf:distribution([c['arena_bytes'][when][space][leaf] for c in cycles]) for leaf in ['reserved_bytes','in_use_bytes','block_count']} for space in ['arena','survivor0','survivor1','longlived','old']} for when in ['before','after']})
 files.add(B/(V+'-'+arm+'-cycles.json'))
proof=dict(scope=scope,summary=summary,source_product_stdout_rss_raw_telemetry_verified=True,limitations='Natural trace logs can perturb residency/pacing. Full-runtime binaries and one plain/trace repetition each do not establish normal-auto peak RSS, an absolute floor, or a performance win.',inputs={n:sha(B/(V+'-'+n+'.json')) for n in ['runs','products','oracle','scope','gc-cycles','entry-cycles','once-cycles']})
(B/(V+'-verification.json')).write_text(json.dumps(proof,indent=2)+'\n')
files.update([B/(V+'-'+n+'.json') for n in ['runs','products','oracle','scope','verification']]);files.update([B/(V+'.exit'),B/(V+'-driver.exit'),B/(V+'-driver.log'),pathlib.Path(__file__),B/'primary-main07-ring-v50.py'])
for ext in ['out','err']:files.add(B/'logs'/(V+'-oracle.'+ext))
m=B/(V+'-evidence-files.json');m.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in sorted(files)},indent=2)+'\n')
with tarfile.open(B/(V+'-evidence.tar.gz'),'w:gz') as t:
 for p in sorted(files|{m}):t.add(p,arcname=str(p.relative_to(B)))
print(json.dumps(proof))
