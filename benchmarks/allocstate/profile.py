#!/usr/bin/env python3
"""Instruction-sampled collector attribution from perf script --no-inline."""
import argparse
import collections
import json
from pathlib import Path
import re
p=argparse.ArgumentParser()
p.add_argument('stacks',type=Path)
a=p.parse_args()
collectors=('::gc::copying::','::gc::cycle::','::gc::oldgen::','::gc::trace::',
            '::gc::mark::','::gc::sweep::','::gc::verify::','::gc::roots::',
            '::gc::layout_slot_visit::')
weight=collections.Counter()
for sample in a.stacks.read_text().split('\n\n'):
    lines=sample.splitlines()
    if not lines: continue
    period=re.search(r'(\d+) instructions:u:',lines[0])
    if not period: continue
    n=int(period[1]);weight['total']+=n
    frames='\n'.join(lines[1:])
    collecting=any(c in frames for c in collectors)
    if collecting: weight['gc']+=n
    if not collecting and any(c in frames for c in ('perry_birth_class+', 'js_inline_arena_state+', 'js_inline_arena_slow_alloc+', 'js_object_alloc')):
        weight['allocation_mutator']+=n
    if 'ValidPointerSet>::contains' in frames:
        weight['valid_pointer_contains']+=n
        if '::hash_one::' in frames or 'sip::' in frames:
            weight['contains_hash']+=n
    top=lines[1] if len(lines)>1 else ''
    if 'js_inline_arena_state+' in top: weight['state_accessor_self']+=n
    if 'perry_birth_class+' in frames: weight['birth_helper']+=n
    if 'js_gc_note_black_birth+' in frames: weight['note_black_birth']+=n
    if 'js_write_barrier_slot+' in frames: weight['slot_barrier']+=n
result={'weighted_instructions':dict(weight),'share_percent':{
    k:100*v/weight['total'] for k,v in weight.items() if k!='total'}}
print(json.dumps(result,indent=2))
