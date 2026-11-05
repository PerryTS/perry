from collections import defaultdict
from causal import expand
NA=2**32-1

def origin(n,why,file='native.rs',gate=None,escape=NA):
 r=dict(node=n,injector=file,line=1,why=why,source=NA,call=NA,escape=escape,owner=NA,gate=gate)
 return dict(r,contexts=[r],whyset={why})

def data():
 return dict(tokens={},functions={},unknown={},sinks=[],escapes=[],fields={},writes=[],reads=[],meta=[dict(objectProto=99,numKey='#num',starKey='*',globalInTop=False)])

def run(d,b):
 adj=defaultdict(set)
 def edge(a,z):
  if a!=z:adj[a].add(z)
 kept,vs,audit,deps=expand(d,{100:{},200:{}},adj,b,edge,lambda *a:None)
 return kept,adj,audit

# Unknown-call escape reinjection is downstream of the callee's TOP.
d=data();d['tokens'][1]=dict(kind='function',function=0,escape='escaped')
d['functions'][0]=dict(params=[200],this=201)
d['unknown'][10]=dict(node=100)
d['sinks']=[dict(node=300,source=100,call=10,parent=NA,why='passed to a call of an unknown function value [call 10]',gate=None)]
d['escapes']=[dict(token=1,source=300,call=10,parent=NA,why='passed to a call of an unknown function value [call 10]',gate=None)]
b=[origin(100,'native result'),origin(200,'param of escaped function','solve/escape.rs',escape=1)]
kept,adj,a=run(d,b)
assert [r['node'] for r in kept]==[100]
assert (1<<32)+1 in adj[100] and 200 in adj[(1<<32)+1]
# An independent native callback keeps its own external input origin.
d['escapes'].append(dict(token=1,source=301,call=11,parent=NA,why='native callback argument',gate=None))
kept,adj,a=run(d,b)
assert {r['node'] for r in kept}=={100,200}
# A written-key gate connects only the TOP-dependent opaque read.
d=data();d['writes']=[dict(token=0,key='foo',source=100,call=NA,parent=NA,numeric=False)]
d['reads']=[dict(token=99,key='foo',isCall=False,id=200,opWrites=False,isGlobal=False,numeric=False)]
b=[origin(100,'native result'),origin(200,'member of a builtin prototype written through top','builtins.rs',dict(token=99,key='foo'))]
kept,adj,a=run(d,b)
assert [r['node'] for r in kept]==[100]
assert next(iter(adj[100])) in adj and 200 in adj[next(iter(adj[100]))]
# An unconditional native result is independent even if the same source
# call also has a conditional written-key summary.
b[1]['contexts'].append(dict(b[1]['contexts'][0],gate=None))
kept,adj,a=run(d,b)
assert {r['node'] for r in kept}=={100,200}
print('PASS: TOP-induced escape is downstream; native callback input is independent; actual native gate distinguishes conditional and unconditional summaries')
