#!/usr/bin/env python3
"""Run the unchanged root gate with indexed, equivalent candidate queries.

The 11929 fixture has 254k instructions. Avoid its sources x instructions x
dominator-depth scan. No source, window, protection, hazard or budget changes.
--validate-indexing compares reference and indexed results on every self-test.
"""
import inspect, re, sys
from collections import defaultdict
from pathlib import Path

gate=Path(__file__).resolve().parents[1]/'scripts/gc_root_dominance_check.py'
ns={'__name__':'lane_root_gate','__file__':str(gate)}
exec(compile(gate.read_text(),str(gate),'exec'),ns)
reference=ns['check_func_statepoints']
original_dominators=ns['dominators']
original_dominates=ns['dominates']

class IndexedDominators(dict):
 def __init__(self, parents):
  super().__init__(parents)
  children=defaultdict(list)
  roots=[]
  for child,parent in parents.items():
   if child==parent:roots.append(child)
   else:children[parent].append(child)
  self.intervals={}
  clock=0
  for root in roots:
   pending=[(root,False)]
   while pending:
    node,exit=pending.pop()
    if exit:self.intervals[node]=(self.intervals[node][0],clock)
    else:
     assert node not in self.intervals,'dominator cycle'
     self.intervals[node]=(clock,None);clock+=1
     pending.append((node,True))
     pending.extend((child,False) for child in reversed(children[node]))
  assert len(self.intervals)==len(parents),'dominator forest incomplete'

def indexed_dominators(f):return IndexedDominators(original_dominators(f))
def indexed_dominates(parents,a,b):
 if a==b:return True
 if not isinstance(parents,IndexedDominators):return original_dominates(parents,a,b)
 ia=parents.intervals.get(a);ib=parents.intervals.get(b)
 return bool(ia and ib and ia[0]<=ib[0]<ia[1])

def indexes(f,phis):
 uses=defaultdict(set);incoming=defaultdict(set)
 order={b:i for i,b in enumerate(f.blocks)}
 for b in f.blocks:
  for ins in f.insns[b]:
   if ns['is_transparent'](ins):continue
   for reg in re.findall(r'%([\w.$]+)',ins.text):uses[reg].add(ins)
 for phi in phis:
  for val,_pred in ns['phi_incoming'](phi):
   val=val.strip()
   if val.startswith('%'):incoming[val[1:]].add(phi)
 return uses,incoming,order

def candidates(index,chain,order):
 values=set()
 for reg in chain:values.update(index.get(reg,()))
 return sorted(values,key=lambda ins:(order[ins.block],ins.idx))

def use_blocks(index,chain,order):
 result={}
 for ins in candidates(index,chain,order):result.setdefault(ins.block,[]).append(ins)
 return result

ns.update(lane_indexes=indexes,lane_candidates=candidates,lane_use_blocks=use_blocks)
source=inspect.getsource(reference)
needle='    phis = [ins for blk in f.blocks for ins in f.insns[blk] if _is_phi(ins)]\n'
assert source.count(needle)==1
source=source.replace(needle,needle+'    lane_uses, lane_phis, lane_order = lane_indexes(f, phis)\n')
needle='            for bb in f.blocks:\n                if not dominates(idom, src.block, bb):\n                    continue\n                for use in f.insns[bb]:\n'
assert source.count(needle)==1
source=source.replace(needle,'            for bb, lane_candidates_in_block in lane_use_blocks(lane_uses, chain, lane_order).items():\n                if not dominates(idom, src.block, bb):\n                    continue\n                for use in lane_candidates_in_block:\n')
needle='chain, as1_chain, phis, poll_reaching,'
assert source.count(needle)==1
source=source.replace(needle,'chain, as1_chain, lane_candidates(lane_phis, chain, lane_order), poll_reaching,')
exec(compile(source,str(Path(__file__).resolve()),'exec'),ns)
indexed=ns['check_func_statepoints']
ns.update(dominators=indexed_dominators,dominates=indexed_dominates)

if '--validate-indexing' in sys.argv:
 sys.argv.remove('--validate-indexing')
 def checked(*args,**kwargs):
  # Run the reference with its own dominance implementation too.
  ns.update(dominators=original_dominators,dominates=original_dominates)
  before=reference(*args,**kwargs)
  ns.update(dominators=indexed_dominators,dominates=indexed_dominates)
  after=indexed(*args,**kwargs)
  def result(rows):
   return [(r.fingerprint,r.kind_class,r.src.block,r.src.idx,r.use.block,r.use.idx) for r in rows]
  assert result(before)==result(after),(args[:2],result(before),result(after))
  return after
 ns['check_func_statepoints']=checked

raise SystemExit(ns['main']())
