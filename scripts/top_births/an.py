#!/usr/bin/env python3
"""Opt-in TOP birth census. No type information or guessed source names enter reachability.
Raw TOP origins are explicit add_top/direct-TOP injections; copy and rule-TOP
edges are solver observations. Escape/native-write dependencies are reported
separately so dependent reinjections cannot silently become independent inputs.
"""
import array,bisect,collections as C,csv,gzip,json,os,pickle,re,sys,time
from pathlib import Path
R=Path(os.environ.get('PERRY_BIRTH_OUTPUT','.')); NA=2**32-1
ROOT=os.environ.get('PERRY_BIRTH_SOURCE_ROOT', '/root/hwp-move/score/p/tssrc/').rstrip('/')+'/'
def failed(kind,value,trace):
 primary=len(sys.argv)>2 and sys.argv[2]=='causal'
 (R/('analysis.failed' if primary else 'raw-analysis.failed')).write_text(str(value)+'\n')
 if primary:(R/'analysis.done').write_text('FAILED census\n')
 sys.__excepthook__(kind,value,trace)
sys.excepthook=failed
name=sys.argv[1] if len(sys.argv)>1 else 'base'
causal=len(sys.argv)>2 and sys.argv[2]=='causal'
prefix='' if causal else 'raw-'
cache=R/(name+'.idx.pkl')
t0=time.time()
def say(*a): print(*a,flush=True)
WARNED=set()
if cache.exists():
 with cache.open('rb') as f: D=pickle.load(f)
else:
 D={k:[] for k in ['injections','edges','escapes','sinks','writes','reads','sites']}
 D.update(analyses=[],units=set(),nodes={},points={},tokens={},calls={},functions={},layouts={},unknown={},copy=array.array('I'),fields={},meta=[])
 for line in open(R/(name+'.dump'),errors='replace'):
  if not line.startswith('D {'):continue
  r=json.loads(line[2:]);k=r['record']
  if r.get('unit')=='<census>':continue
  D['units'].add(r.get('unit'))
  if k=='analysis':D['analyses'].append(r)
  elif k=='node':
   escaped_tokens=tuple(t for t in r['tokens'] if t!=0 and D['tokens'].get(t,{}).get('escape'))
   if escaped_tokens:D['points'][r['id']]=escaped_tokens
   if r['top']:D['nodes'][r['id']]={a:r[a] for a in ['label','topRoot','topOrigin']}
  elif k=='copy-edges':
   for t in r['targets']:D['copy'].extend((r['source'],t))
  elif k=='token': D['tokens'][r['id']]={a:r.get(a) for a in ['kind','name','layout','function','escape']}
  elif k=='function':D['functions'][r['id']]=r
  elif k=='layout':D['layouts'][r['id']]=r
  elif k=='site':D['sites'].append(r)
  elif k=='top-field': D['fields'][r['node']]=r['token']
  elif k=='unknown-call':D['unknown'][r['call']]=r
  elif k.startswith('birth-'):
   if k[6:] in ('arrival','opener'):continue # chain.py/openers.py records: the birth analysis does not read them
   target={'injection':'injections','edge':'edges','escape':'escapes','sink':'sinks','write':'writes','read':'reads','call':'calls','meta':'meta'}.get(k[6:])
   if target is None:
    if k not in WARNED:WARNED.add(k);print('an.py: skipping unknown record kind',k,file=sys.stderr)
    continue
   if target=='calls':D[target][r['id']]=r
   else:D[target].append(r)
 with cache.open('wb') as f:pickle.dump(D,f,protocol=5)
say('indexed', {k:len(v) for k,v in D.items()}, 'seconds',round(time.time()-t0,1))
assert not any(r.get('aborted') for r in D['analyses']), 'aborted solver: census is invalid'
assert len(D['units'])==1, ('node IDs must not be merged across analysis units',D['units'])
# Load identities before assigning root bits. Only roots that reach an
# unproven blocking operand need a bit; zero-mass origins are still listed.
ledger={}
for r in csv.DictReader(gzip.open(R/(name+'.ledger.tsv.gz'),'rt'),delimiter='\t'):
 k=tuple(r[a] for a in ['file','start','end','kind','key']);ledger[k]=r
km={'value-call':'call'};by_span=C.defaultdict(list);by_start=C.defaultdict(list)
for k in ledger:
 by_span[k[:4]].append(k);by_start[k[0],k[1],k[3]].append(k)
identity_stats=C.Counter()
def identity(r):
 file=r['file'].removeprefix(ROOT);kind=km.get(r['kind'],r['kind'])
 k=(file,str(r['start']),str(r['end']),kind,r['key'])
 if r['start']==0:return k
 span=by_span.get(k[:4],())
 if len(span)==1:return span[0]
 if k in ledger:return k
 cand=[x for x in by_start.get((file,str(r['start']),kind),()) if x[4]==r['key'] or kind=='new']
 return cand[0] if len(cand)==1 else k
N=D['nodes'];adj=C.defaultdict(set)
def edge(a,b):
 if a!=b:adj[a].add(b)
for a,b in zip(D['copy'][::2],D['copy'][1::2]):
 if a in N and b in N:edge(a,b)
for r in D['edges']:
 if r['source'] in N and r['target'] in N:edge(r['source'],r['target'])
# Every explicit non-copy injection is a birth in the raw solver graph.
B={}
for r in D['injections']:
 k=(r['node'],r['injector'],r['line'])
 b=B.setdefault(k,dict(r,contexts=[],whyset=set()))
 b['contexts'].append(r);b['whyset'].add(r['why'])
# Backward-compatible audit of older dumps without constructor capture.
# These origins are explicit, never inferred from a predecessor label.
for n,r in N.items():
 if r['label'] in ('TOPVAL','THROWN') and not any(k[0]==n for k in B):
  if n<20:
   B[n,'<model-initialization>',0]=dict(node=n,injector='<model-initialization>',line=0,
      why='initial shared unknown value' if r['label']=='TOPVAL' else 'thrown value (runtime errors)',
      contexts=[],whyset={r['label']})
B=sorted(B.values(),key=lambda r:(r['injector'],r['line'],r['node']))
vertices=set(N);audit={}
if causal:
 from causal import expand
 inst_path=R/'instance-origins.json.gz'
 D['instances']={int(k):v for k,v in json.load(gzip.open(inst_path,'rt')).items()} if inst_path.exists() else {}
 B,vertices,audit,dependent=expand(D,N,adj,B,edge,say)
 (R/'dependent-reinjections.json').write_text(json.dumps([{k:b[k] for k in ['node','injector','line','why','dependencies']} for b in dependent],indent=1)+'\n')
# The requested unit is a birth NODE, not an injector location. Keep every
# injecting rule as provenance but assign exactly one origin bit per node.
by_node={}
for b in B:
 site={k:b[k] for k in ['injector','line','why']}
 site['reasons']=sorted(b['whyset'])
 if b['node'] not in by_node:
  by_node[b['node']]=dict(b,injection_sites=[site],contexts=list(b['contexts']),whyset=set(b['whyset']))
 else:
  v=by_node[b['node']];v['injection_sites'].append(site)
  v['contexts'].extend(b['contexts']);v['whyset'].update(b['whyset'])
  if b.get('association_gap'):v['association_gap']=True
B=list(by_node.values())
# SCCs: iterative Tarjan over observed graph, no recursion limit or model inference.
def scc(adj,vertices):
 index={};low={};on=set();stack=[];comp={};sizes=[];seq=0
 for start in vertices:
  if start in index:continue
  index[start]=low[start]=seq;seq+=1;stack.append(start);on.add(start)
  frames=[(start,iter(adj.get(start,())))]
  while frames:
   v,it=frames[-1]
   try:w=next(it)
   except StopIteration:
    frames.pop()
    if frames:low[frames[-1][0]]=min(low[frames[-1][0]],low[v])
    if low[v]==index[v]:
     count=0;cid=len(sizes)
     while True:
      w=stack.pop();on.remove(w);comp[w]=cid;count+=1
      if w==v:break
     sizes.append(count)
    continue
   if w not in index:
    index[w]=low[w]=seq;seq+=1;stack.append(w);on.add(w);frames.append((w,iter(adj.get(w,()))))
   elif w in on:low[v]=min(low[v],index[w])
 return comp,sizes
say('SCC start',len(N),sum(map(len,adj.values())),len(B))
comp,sizes=scc(adj,vertices)
dag=[set() for _ in sizes];reverse=[[] for _ in sizes];indeg=[0]*len(sizes)
for a,targets in adj.items():
 ca=comp[a]
 for b in targets:
  cb=comp[b]
  if ca!=cb and cb not in dag[ca]:dag[ca].add(cb);reverse[cb].append(ca);indeg[cb]+=1
useful=set()
for r in D['sites']:
 k=identity(r)
 if k not in ledger or ledger[k]['verdict']!='unproven':continue
 for role in ['callee','receiver','index']:
  n=r.get(role)
  if n in N:useful.add(comp[n])
  if causal and role=='receiver':
   for t in D.get('points',{}).get(n,()):
    et=(1<<32)+t
    if et in comp:useful.add(comp[et])
q=C.deque(useful)
while q:
 for c in reverse[q.popleft()]:
  if c not in useful:useful.add(c);q.append(c)
# Compact root positions: a high numbered dead origin must not allocate a
# 70-KB one-bit Python integer at every unrelated component.
impact=[b for b in B if comp[b['node']] in useful]
B=impact+[b for b in B if comp[b['node']] not in useful]
active=len(impact);del impact,reverse
say('target slice',len(useful),'components','active roots',active,'all roots',len(B))
mask=[0]*len(sizes);covered=bytearray(len(sizes))
for i,b in enumerate(B):
 c=comp[b['node']];covered[c]=1
 if i<active:mask[c]|=1<<i
q=C.deque(i for i,d in enumerate(indeg) if not d)
while q:
 a=q.popleft();bits=mask[a]
 for b in dag[a]:
  covered[b]|=covered[a]
  if b in useful:mask[b]|=bits
  indeg[b]-=1
  if indeg[b]==0:q.append(b)
say('SCC done',len(sizes),'max',max(sizes),'uncovered_top',sum(not covered[comp[n]] for n in N))
# Identity ledger, not the explanatory last-blocker seed. All unknown site
# roles contribute; receiver escape/shape blockers are separately counted.
site_masks=C.defaultdict(int);site_sources=C.defaultdict(set);node_sites=C.defaultdict(list);call_sites={}
for r in D['sites']:
 file=r['file'].removeprefix(ROOT)
 k=identity(r)
 for role in ['result','callee','receiver','index']:
  n=r.get(role)
  if isinstance(n,int):node_sites[n].append((0 if role=='result' else 1,r))
 if r.get('call') is not None:call_sites[r['call']]=r
 if k not in ledger or ledger[k]['verdict']!='unproven':continue
 # Include every blocking TOP role (not just the chosen topRoot).
 for role in ['callee','receiver','index']:
  n=r.get(role)
  if n in N:
   site_masks[k]|=mask[comp[n]];site_sources[k].add(n)
  if causal and role=='receiver':
   for t in D.get('points',{}).get(n,()):
    et=(1<<32)+t
    if et in comp:
     site_masks[k]|=mask[comp[et]];site_sources[k].add(et)
 # Shape escape can block a receiver without literal TOP. That is not a
 # TOP-valued role, so it is not silently added to this raw TOP census.
counts=C.Counter();sole=C.Counter();hist=C.Counter();planes=[]
# Bit-sliced integer addition sums every origin's mass in parallel. This
# avoids subtracting one bit at a time from a 200,000-bit dense mask.
for bits,mass in C.Counter(site_masks.values()).items():
 hist[bits.bit_count()]+=mass
 if bits and bits&(bits-1)==0:sole[bits.bit_length()-1]+=mass
 shift=0
 while mass:
  if mass&1:
   carry=bits;k=shift
   while carry:
    if k>=len(planes):
     planes.extend([0]*(k+1-len(planes)));planes[k]=carry;break
    old=planes[k];planes[k]=old^carry;carry&=old;k+=1
  shift+=1;mass>>=1
bit_positions=[[i for i in range(8) if v&(1<<i)] for v in range(256)]
totals=[0]*active
for p,plane in enumerate(planes):
 for byte,value in enumerate(plane.to_bytes((active+7)//8,'little')):
  for bit in bit_positions[value]:
   i=byte*8+bit
   if i<active:totals[i]+=1<<p
counts.update({i:n for i,n in enumerate(totals) if n})
say('mass counts done',len(site_masks),'identities',len(counts),'nonzero births')
# Source locations use exact source calls/results first; shared summaries
# can have multiple expressions. No byte offsets are mistaken for lines.
source_cache={};expr_cache={}
ast_path=R/'source-ast.json'
ast=json.load(open(ast_path)) if ast_path.exists() else {}
layout_allow=ast.get('entries',{}) | ast.get('expressions',{})
inst_path=R/'instance-origins.json.gz'
inst=D.get('instances') or ({int(k):v for k,v in json.load(gzip.open(inst_path,'rt')).items()} if inst_path.exists() else {})
def original(n):
 seen=set()
 while n in inst and n not in seen:
  seen.add(n);n=inst[n]
 return n
def expr(r):
 key=(r['file'],r['start'],r['end'])
 if key not in expr_cache:expr_cache[key]=expr_uncached(r)
 return expr_cache[key]
def expr_uncached(r):
 p=r['file'];s=r['start'];e=r['end']
 try:
  if p not in source_cache:
   data=Path(p).read_bytes();source_cache[p]=(data,[0]+[i+1 for i,b in enumerate(data) if b==10])
  data,starts=source_cache[p];line=bisect.bisect_right(starts,max(0,s-1));text=data[max(0,s-1):max(s-1,e-1)].decode(errors='replace')
  return p.removeprefix(ROOT)+':'+str(line),re.sub(r'\s+',' ',text)[:180]
 except OSError:return p+':?', '<source unavailable>'
def category(b):
 w=b['why'];f=b['injector']
 if 'escape.rs' in f or w.startswith(('param of escaped function','this of escaped function')):return 'external input after independent escape'
 if 'builtin prototype written' in w:return 'builtin opaque read after native write'
 if 'written builtin function' in w:return 'builtin opaque read after bf write'
 if w.startswith('native module ref '):return 'module boundary: native module reference'
 if 'another unit' in w or 'unresolved export' in w or 're-export' in w:return 'module boundary'
 if 'eval' in w or 'dynamic code' in w:return 'dynamic-code summary'
 if w.startswith('implicit native receiver'):return 'external input: implicit native receiver'
 if 'native accessor lookup' in w:return 'builtin opaque read: native accessor lookup'
 if 'result' in w or '`new`' in w or 'instance' in w:return 'native/unmodeled result'
 if 'global' in w or 'thrown' in w or 'receiver from unmodeled' in w:return 'external input'
 if w=='accessor of unknown function value':return 'accessor of unknown function value'
 if w.startswith('extern value '):return 'external input: native global value'
 if w.startswith('spread of an unmodeled object'):return 'opaque spread: unmodeled source'
 if w.startswith('module loaded by a computed specifier'):return 'module boundary: computed specifier'
 if w.startswith('unresolved scope name '):return 'unresolved lexical binding'
 if w.startswith('process.'):return 'builtin opaque read: process member'
 if f.endswith('/gen.rs') and w=='direct TOP token':return 'external input: shared unknown value'
 if f.endswith('/gen_rest/reflection.rs') and w=='direct TOP token':return 'module boundary: dynamic-import runtime fallback'
 return 'other: '+f.rsplit('/',1)[-1]+':'+str(b['line'])
paramloc={}
for f,r in D['functions'].items():
 for n in r['params']+[r['this']]:paramloc[n]=r
escape_locations=C.defaultdict(dict)
for r in D['escapes']:
 call=D['calls'].get(r['call'],{});sc=call.get('sourceCall',r['call'])
 if sc in call_sites:
  site=call_sites[sc];escape_locations[r['token']][site['file'],site['start'],site['end']]=site
unknown_locations=C.defaultdict(dict)
for u in D['unknown'].values():
 sc=u.get('source');site=call_sites.get(sc)
 if site is not None:unknown_locations[u.get('node')][site['file'],site['start'],site['end']]=site
def module_locations(b):
 if not b['why'].startswith('native module ref '):return []
 name=b['why'][len('native module ref '):]
 pattern=re.compile(rb'(?:require|import)\s*\(\s*[\"\'](?:node:)?'+re.escape(name.encode())+rb'[\"\']\s*\)')
 out=[];seen=set()
 for r in b['contexts']:
  fn=D['functions'].get(r['owner'])
  if not fn or not fn['file']:continue
  file=fn['file']
  try:data=Path(file).read_bytes()
  except OSError:continue
  lo=max(0,fn['start']-1);hi=fn['end']-1 if fn['end']>fn['start'] else len(data)
  for m in pattern.finditer(data,lo,hi):
   k=(file,m.start()+1,m.end()+1)
   if k not in seen:seen.add(k);out.append(expr(dict(file=file,start=k[1],end=k[2])))
 return out
for i,b in enumerate(B):
 options=[]
 for r in b['contexts']:
  call=D['calls'].get(r['call'],{});sc=call.get('sourceCall',r['call'])
  if sc in call_sites:options.append((0,call_sites[sc]))
 for token in {r['escape'] for r in b['contexts'] if r['escape']!=NA}:
  options.extend((0,s) for s in escape_locations.get(token,{}).values())
 for n in {b['node'],original(b['node'])}:
  options.extend((0,r) for r in unknown_locations.get(n,{}).values())
 on=original(b['node'])
 if on in paramloc and paramloc[on]['start']:
  options.append((1,paramloc[on]))
 if b['node'] in D['fields']:
  tok=D['tokens'].get(D['fields'][b['node']],{})
  layout=D['layouts'].get(tok.get('layout'))
  if layout and layout['start']:options.append((1,layout))
 if not any(p<=1 for p,r in options):
  options += [(p+2,r) for p,r in node_sites.get(b['node'],[])]
  if on!=b['node']:options += [(p+2,r) for p,r in node_sites.get(on,[])]
 best=min((p for p,r in options),default=9)
 options=[(p,r) for p,r in options if p==best]
 seen=set();locs=[]
 for _,r in sorted(options,key=lambda x:(x[0],x[1]['file'],x[1]['start'])):
  v=expr(r)
  if v not in seen:seen.add(v);locs.append(v)
 if not locs:locs=[('<model>' if b['injector']=='<model-initialization>' else '<unmapped>',b['why'])]
 source_kind={0:'source-call',1:'declaration-or-layout',2:'result-expression',3:'use-only'}.get(min((p for p,r in options),default=9),'unmapped')
 hinted=[];hint_kind=None
 if b['why'].startswith('native module ref '):
  hinted=ast.get('modules',{}).get(b['why'][len('native module ref '):],[]);hint_kind='source-module-load'
 elif b['why'].startswith('extern value '):
  hinted=ast.get('globals',{}).get(b['why'][len('extern value '):],[]);hint_kind='source-native-value'
 elif b['why'].startswith('result of native module '):
  hinted=ast.get('methods',{}).get(b['why'][len('result of native module '):],[]);hint_kind='source-native-call'
 if hinted:
  locs=[(r['source'],r['expression']) for r in hinted];source_kind=hint_kind
 elif ast:
  allow=layout_allow if source_kind=='declaration-or-layout' else ast.get('expressions',{})
  good=[v for v in locs if v[0]+'\t'+v[1] in allow]
  if good:locs=good
  else:
   locs=[(b['injector']+':'+str(b['line']),'model injection: '+b['why'])];source_kind='model-generated'
 else:
  module_locs=module_locations(b)
  if module_locs:locs=module_locs;source_kind='source-module-load'
 b.update(id=i,kind=' | '.join(sorted({category(x) for x in b['injection_sites']})),total=counts[i],sole=sole[i],locations=locs,source_kind=source_kind)
 b['switch']=f"{b['injector']}:{b['line']}:{b['node']}" if len(b['injection_sites'])==1 else ''
quality={'source-call':0,'source-module-load':0,'source-native-value':0,'source-native-call':0,'model-generated':5,'result-expression':1,'declaration-or-layout':2,'use-only':3,'unmapped':4}
sole_order=lambda b:(-b['sole'],-b['total'],quality[b['source_kind']],b['locations'][0][0],b['id'])
total_order=lambda b:(-b['total'],-b['sole'],quality[b['source_kind']],b['locations'][0][0],b['id'])
fields=['id','node','kind','sole','total','source','expression','injector','reason','switch','source_count','source_kind']
with (R/(prefix+'births.tsv')).open('w') as out:
 w=csv.DictWriter(out,fields,delimiter='\t');w.writeheader()
 for b in sorted(B,key=sole_order):
  w.writerow(dict(id=b['id'],node=b['node'],kind=b['kind'],sole=b['sole'],total=b['total'],source=b['locations'][0][0],expression=b['locations'][0][1],injector=' | '.join(x['injector']+':'+str(x['line']) for x in b['injection_sites']),reason=' | '.join(sorted(b['whyset'])),switch=b['switch'],source_count=len(b['locations']),source_kind=b['source_kind']))
mask_ids={bits:i for i,bits in enumerate(sorted(set(site_masks.values())))}
with gzip.open(R/(prefix+'birth-masks.tsv.gz'),'wt',compresslevel=1) as out:
 w=csv.writer(out,delimiter='\t');w.writerow(['mask','births_hex'])
 for bits,i in mask_ids.items():w.writerow([i,format(bits,'x')])
with gzip.open(R/(prefix+'birth-sites.tsv.gz'),'wt',compresslevel=1) as out:
 w=csv.writer(out,delimiter='\t');w.writerow(['file','start','end','kind','key','mask','sole'])
 for k,bits in sorted(site_masks.items()):w.writerow([*k,mask_ids[bits],int(bits.bit_count()==1)])
summary={'audit':audit,'births':len(B),'independent_injection_locations':sum(len(b['injection_sites']) for b in B),'top_nodes':len(N),'edges':sum(map(len,adj.values())),'components':len(sizes),'max_component':max(sizes),
 'uncovered_top_nodes':sum(not covered[comp[n]] for n in N),'active_births':active,'target_components':len(useful),'unproven':sum(r['verdict']=='unproven' for r in ledger.values()),
 'unproven_with_top_roles':len(site_masks),'unproven_with_reached_top_roles':sum(bool(v) for v in site_masks.values()),
 'sole_sites':sum(sole.values()),'kind_births':dict(C.Counter(k for b in B for k in b['kind'].split(' | '))),'source_kinds':dict(C.Counter(b['source_kind'] for b in B)),
 'top_sole':[b['id'] for b in sorted(B,key=sole_order)[:25]],
 'top_total':[b['id'] for b in sorted(B,key=total_order)[:25]],
 'multiplicity_histogram':dict(sorted(hist.items())),'association_gap_births':[dict(node=b['node'],total=b['total'],sole=b['sole'],injector=b['injector'],reason=b['why']) for b in B if b.get('association_gap')]}
(R/(prefix+'birth-summary.json')).write_text(json.dumps(summary,indent=2)+'\n')
with gzip.open(R/(prefix+'birth-origins.json.gz'),'wt',compresslevel=1) as f:json.dump([{k:v for k,v in b.items() if k!='contexts'} for b in B],f,default=lambda x:sorted(x))
with gzip.open(R/(prefix+'birth-locations.tsv.gz'),'wt',compresslevel=1) as f:
 w=csv.writer(f,delimiter='\t');w.writerow(['birth','source','expression'])
 for b in B:
  for loc,text in b['locations']:w.writerow([b['id'],loc,text])
say('summary',summary)
for key in ['top_sole','top_total']:
 say(key)
 for i in summary[key]:
  b=B[i];say(i,b['sole'],b['total'],b['kind'],*b['locations'][0])
say('DONE seconds',round(time.time()-t0,1))

with gzip.open(R/(prefix+'birth-injectors.tsv.gz'),'wt',compresslevel=1) as f:
 w=csv.writer(f,delimiter='\t');w.writerow(['birth','node','injector','kind','reasons'])
 for b in B:
  for x in b['injection_sites']:w.writerow([b['id'],b['node'],x['injector']+':'+str(x['line']),category(x),' | '.join(x['reasons'])])
