#!/usr/bin/env python3
"""Reconstruct final evidence from independent raw trials, avoiding batch JSON overwrites."""
import argparse
import ast
import json
from pathlib import Path
import re
import statistics

PROGRAMS=['tsc','zod5k','qsparse','qsstr','commander','commander-construct','hello','fastify',
          'effect','schema-record','buffer_heavy','worker_heavy','class_getter','object_array',
          'class_virtual','object_spread','binarytrees_plain','binarytrees_arena','root-survival']
MICROS={'class_getter','object_array','class_virtual','object_spread'}
p=argparse.ArgumentParser();p.add_argument('--hostdir',type=Path,required=True);a=p.parse_args();root=a.hostdir
status={}
for name in ('compile-trees-main.log','compile-main.log','compile-head.log','compile-trees-head-final.log','compile-fastify-final.log'):
    for line in (root/name).read_text().splitlines():
        match=re.fullmatch(r'(\S+) (main|head) (\{.*\})',line)
        if match: status.setdefault(match[1],{})[match[2]]=ast.literal_eval(match[3])
def trials(name,arm,low=False):
    directory=root/'measure/trials'
    if name in MICROS and not low: directory=root/'measure/micro-high-trials'
    rows=[]
    for trial in range(5):
        prefix=directory/f'{name}-{arm}-{trial}'
        raw=prefix.with_suffix('.stat').read_text()
        counter=re.search(r'^(\d+);;instructions:u;',raw,re.M)
        if counter is None: raise RuntimeError(f'missing counter: {prefix}')
        rows.append({'instructions':int(counter[1]),'rss_kb':int(prefix.with_suffix('.rss').read_text().strip())})
    return rows
results={}
for name in PROGRAMS:
    results[name]={}
    for arm in ('main','head'):
        if status[name][arm]['status']!='pass': raise RuntimeError(f'{name}/{arm} did not pass')
        rows=trials(name,arm)
        result={'trials':rows,'medians':{k:statistics.median(r[k] for r in rows) for k in rows[0]},
                'text':status[name][arm]['text']}
        trace=(root/'measure'/arm/f'{name}.gc.err').read_text().splitlines()
        events=[json.loads(line) for line in trace if line.startswith('{')]
        cycles=[e for e in events if e.get('event')=='gc_cycle']
        result['gc']={kind:sum(e['collection_kind']==kind for e in cycles) for kind in ('full','minor')}
        if cycles:
            result['gc']['max_reserved_bytes']=max(e['arena_bytes']['before']['total_reserved_bytes'] for e in cycles)
            result['gc']['max_live_allocated_bytes']=max(e['arena_bytes']['before']['total_live_allocated_bytes'] for e in cycles)
            result['gc']['promoted_bytes']=sum(e['copying_nursery']['promoted_bytes'] for e in cycles)
        if name in MICROS:
            low=trials(name,arm,True)
            low_median=statistics.median(r['instructions'] for r in low)
            result['low_trials']=low
            result['instructions_per_iteration']=(result['medians']['instructions']-low_median)/150000
        results[name][arm]=result
(root/'measure/final-results.json').write_text(json.dumps(results,indent=2))
for name,arms in results.items():
    b,h=arms['main'],arms['head'];bi=b['medians']['instructions'];hi=h['medians']['instructions']
    print(f"{name:20} {bi:14,d} {hi:14,d} {100*(hi/bi-1):+8.4f}%  RSS {b['medians']['rss_kb']:7,d}/{h['medians']['rss_kb']:7,d} KB  .text {b['text']:9,d}/{h['text']:9,d}  GC {b['gc']['full']}/{b['gc']['minor']} {h['gc']['full']}/{h['gc']['minor']}")
