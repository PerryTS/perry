"""Expanded observed TOP graph. Applied by an.py before SCC propagation.
Only actual copy/rule edges, recorded escape/sink relations and native-key
write/read matches are used. Independent native effects retain explicit
injection births; dependent reinjections do not get a new root bit.
"""
def expand(D,N,adj,B,edge,say):
 import collections as C,re
 NA=2**32-1;EBASE=1<<32;WBASE=2<<32
 escaped={t for t,r in D['tokens'].items() if r.get('escape')}
 E=lambda t:EBASE+t
 allv=set(N)|{E(t) for t in escaped}
 stats=C.Counter();independent=set();events=C.defaultdict(list);sinks=C.defaultdict(list)
 for r in D['sinks']:sinks[r['node'],r['why']].append(r)
 for r in D['escapes']:
  if r['token'] in escaped:events[r['token']].append(r)
 unknown=D['unknown'];callread=C.defaultdict(set);noderead=C.defaultdict(set)
 gates={};gate_deps=C.defaultdict(set);gate_ind=set();gate_keys={}
 for r in D['writes']:
  key=(r['token'],r['key'])
  if key not in gates:gates[key]=WBASE+len(gates);gate_keys[gates[key]]=key
  g=gates[key];allv.add(g)
  if r['parent'] in escaped:edge(E(r['parent']),g)
  elif r['token']==0 and r['source'] in N:edge(r['source'],g)
  elif r['token']==0:stats['top_writer_missing_top_source']+=1
  else:gate_ind.add(g)
 meta=D['meta'][0] if D['meta'] else {}
 star=meta.get('starKey','*');num=meta.get('numKey','#num');op=meta.get('objectProto')
 global_in_top=meta.get('globalInTop',False)
 gate_reads=C.defaultdict(set)
 write_numeric={(r['token'],r['key']):r.get('numeric',False) for r in D['writes']}
 def match(w,k,wn,kn):
  return w is None or w==k or k==star or (w==num and (kn or k=='length')) or (k==num and wn)
 queries={(r['token'],r['key'],r.get('opWrites',False),r.get('numeric',False),r.get('isGlobal',False)):r for r in D['reads']}
 for r in queries.values():
  for (t,k),g in gates.items():
   top=t==0 and (not r.get('isGlobal',False) or global_in_top)
   own=t==r['token'] or (r.get('opWrites',False) and t==op)
   if (top or own) and match(k,r['key'],write_numeric[t,k],r.get('numeric',False)):
    gate_reads[r['token'],r['key']].add(g)
 def calls_from(r):
  ids=[]
  if r.get('call',NA)!=NA:ids.append(r['call'])
  ids.extend(int(x) for x in re.findall(r'\[call (\d+)\]',r.get('why','')))
  return ids
 def dependencies(r,why=None):
  why=r.get('why','') if why is None else why
  parent=r.get('parent',NA)
  if parent in escaped:return {E(parent)},False
  gate=r.get('gate')
  if gate is not None:
   deps=gate_reads.get((gate['token'],gate['key']),set())
   if not deps:stats['recorded_gate_without_writer']+=1
   return deps,not bool(deps)
  src=r.get('source',NA)
  if ('unknown function' in why or 'unknown constructor' in why) and 'opaque builtin' not in why:
   if src in N:return {src},False
   # Use the old first-callee record only when the event has no actual
   # source at all; another receiver's first callee must not replace it.
   if src==NA:
    deps={unknown[c].get('node') for c in calls_from(r) if c in unknown and unknown[c].get('node') in N}
    if deps:stats['unknown_call_source_fallback']+=1;return deps,False
  if ('unmodeled value' in why or 'unmodeled object' in why) and src in N:
   return {src},False
  return set(),True
 for t,rows in events.items():
  dst=E(t)
  for r in rows:
   if r['parent'] in escaped:deps={E(r['parent'])};ind=False
   elif (r['source'],r['why']) in sinks:
    deps=set();ind=False
    for s in sinks[r['source'],r['why']]:
     ds,ins=dependencies(s,r['why']);deps|=ds;ind|=ins
   else:deps,ind=dependencies(r)
   for src in deps:edge(src,dst)
   if ind:independent.add(dst)
 # Propagate "can escape without a TOP prerequisite" through escape/flag
 # vertices only. TOP nodes do not become independent merely by this pass.
 independent|=gate_ind
 q=C.deque(independent)
 while q:
  src=q.popleft()
  for dst in adj.get(src,()):
   if dst>=EBASE and dst not in independent:
    independent.add(dst);q.append(dst)
 # Function parameter and object-field injections can occur after their
 # escape transfer returned; recover that same token via structural records.
 param={};ftokens=C.defaultdict(set)
 for t,v in D['tokens'].items():
  if v.get('function') is not None and t in escaped:ftokens[v['function']].add(t)
 for f,r in D['functions'].items():
  for n in r['params']+[r['this']]:param[n]=ftokens.get(f,set())
 instances=D.get('instances',{})
 def original(n):
  seen=set()
  while n in instances and n not in seen:seen.add(n);n=instances[n]
  return n
 keep=[];dependent=[]
 for b in B:
  deps=set();retain=False
  escape_inject=('escape.rs' in b['injector'] or b['why'].startswith(('param of escaped function','this of escaped function','property of an escaped function','field of escaped ','stored into an escaped ','constructor args from unknown code')))
  opaque='builtin prototype written' in b['why']
  native_fallback=('written primitive method' in b['why'] or 'written intrinsic key' in b['why'] or 'argument of native ' in b['why'])
  for r in b['contexts']:
   if r['why'].startswith('implicit native receiver'):
    if r.get('gate') is not None:
     gate=r['gate'];ds=gate_reads.get((gate['token'],gate['key']),set());deps|=ds
     retain|=not bool(ds) or bool(ds&independent)
    elif r['escape'] in escaped:
     deps.add(E(r['escape']));retain|=E(r['escape']) in independent
    else:retain=True
   elif escape_inject:
    tokens={r['escape']} if r['escape'] in escaped else set(param.get(original(b['node']),()))
    if not tokens:
     t=D['fields'].get(b['node'])
     if t in escaped:tokens={t}
    if tokens:
     for t in tokens:
      deps.add(E(t));retain|=E(t) in independent
      if t not in events:retain=True;stats['escape_origin_without_event_retained']+=1
    else:
     retain=True;stats['escape_injection_missing_escape_token']+=1
     b['association_gap']=True
   elif r.get('gate') is not None:
    gate=r['gate'];ds=gate_reads.get((gate['token'],gate['key']),set());deps|=ds
    if not ds:retain=True;stats['injection_gate_without_writer']+=1
    retain|=bool(ds&independent)
   elif opaque:
    retain=True;stats['opaque_injection_without_observed_gate']+=1
   else:retain=True
  if not b['contexts']:retain=True
  for src in deps:edge(src,b['node'])
  b['dependencies']=sorted(deps);b['independent']=retain
  if retain:keep.append(b)
  else:dependent.append(b)
 stats['raw_injections']=len(B);stats['retained_births']=len(keep);stats['dependent_reinjections']=len(dependent)
 stats['escape_vertices']=len(escaped);stats['independent_escape_or_write_vertices']=len(independent);stats['native_write_vertices']=len(gates)
 # Every known escaped token must have at least one recorded reason.
 stats['escaped_without_recorded_event']=len(escaped-set(events))
 say('CAUSAL AUDIT',dict(stats))
 return keep,allv,dict(stats),dependent
