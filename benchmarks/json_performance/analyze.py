#!/usr/bin/env python3
"""Aggregate raw trials without discarding slow successful samples."""
import collections,csv,json,statistics
from pathlib import Path
ROOT=Path(__file__).resolve().parent
OUT=ROOT/'results'
def read(name):return [json.loads(s) for s in (OUT/(name+'.jsonl')).read_text().splitlines()]
def med(rows,key):return statistics.median(r[key] for r in rows)
fixtures={f['name']:f for f in json.loads((OUT/'fixtures.json').read_text())}
verified=read('verify')
assert len(verified)==150 and all(r['correct'] for r in verified)
output_bytes={(r['fixture'],r['operation']):r['verify_bytes'] for r in verified if r['engine']=='node'}
summary=[]
for phase in ['timing','memory','tape']:
    groups=collections.defaultdict(list)
    for r in read(phase):
        assert 'error' not in r,r
        groups[(r['fixture'],r['operation'],r['engine'],r['tape'],r['iterations'])].append(r)
    for (fixture,op,engine,tape,n),rows in groups.items():
        assert len(rows)==3,(phase,fixture,op,engine,len(rows))
        cpu=[r['cpu_ms']/n for r in rows];wall=[r['wall_ms']/n for r in rows]
        size=output_bytes.get((fixture,'stringify'),fixtures[fixture]['bytes']) if 'stringify' in op else fixtures[fixture]['bytes']
        summary.append(dict(phase=phase,fixture=fixture,operation=op,engine=engine,tape=tape,iterations=n,
                            input_bytes=fixtures[fixture]['bytes'],work_bytes=size,cpu_ms=statistics.median(cpu),
                            cpu_min_ms=min(cpu),cpu_max_ms=max(cpu),wall_ms=statistics.median(wall),
                            wall_min_ms=min(wall),wall_max_ms=max(wall),throughput_mib_s=size/1048576/(statistics.median(wall)/1000),
                            rss_before_mib=med(rows,'rss_before')/1048576,rss_after_mib=med(rows,'rss_after')/1048576,
                            peak_rss_mib=med(rows,'peak_rss')/1048576,peak_footprint_mib=med(rows,'peak_footprint')/1048576))
with (OUT/'summary.csv').open('w') as f:
    writer=csv.DictWriter(f,fieldnames=list(summary[0]),lineterminator="\n");writer.writeheader();writer.writerows(summary)
(OUT/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
lookup={(r['fixture'],r['operation'],r['engine']):r for r in summary if r['phase']=='timing'}
lines=['All values are medians of three fresh processes. CPU is user + system process CPU, including helper threads and GC. RSS is whole-process peak.','',
       '| Fixture / operation | Input bytes | CPU µs: Perry / Node / Bun | Wall µs: Perry / Node / Bun | Peak RSS MiB: Perry / Node / Bun |',
       '|---|---:|---:|---:|---:|']
for fixture,f in fixtures.items():
    for op in f['operations']:
        rows=[lookup[(fixture,op,e)] for e in ['perry','node','bun']]
        fmt=lambda key,scale: ' / '.join(f'{r[key]*scale:,.3f}' for r in rows)
        lines.append(f"| {fixture} / {op} | {f['bytes']:,} | {fmt('cpu_ms',1000)} | {fmt('wall_ms',1000)} | {fmt('peak_rss_mib',1)} |")
(OUT/'full-tables.md').write_text('\n'.join(lines)+'\n')
# Compact exportable charts, use log axes because nanosecond and multi-ms costs differ.
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np
plt.rcParams.update({'font.family':'DejaVu Sans','font.size':10})
colors={'perry':'#247577','node':'#617b38','bun':'#b47138'}
small=['tiny_object','small_record','object_1k']
big=['records_array_1m','records_object_1m','long_string_1m','numbers_1m','wide_1m']
labels={'tiny_object':'7 B object','small_record':'109 B record','object_1k':'1 KB object','records_array_1m':'0.89 MB array*','records_object_1m':'0.89 MB envelope','long_string_1m':'1.05 MB string','numbers_1m':'1.07 MB numbers*','wide_1m':'50,000-key object'}
fig,axes=plt.subplots(2,2,figsize=(13,9),layout='constrained')
for row,names in enumerate([small,big]):
    for col,op in enumerate(['parse','stringify']):
        ax=axes[row,col];ys=np.arange(len(names))
        for i,e in enumerate(['perry','node','bun']):
            values=[lookup[(f,op,e)]['cpu_ms']*1000 for f in names]
            ax.barh(ys+(i-1)*.23,values,height=.21,label=e.capitalize(),color=colors[e])
        ax.set_yticks(ys,[labels[f] for f in names]);ax.invert_yaxis();ax.set_xscale('log')
        ax.set_xlabel('CPU µs per call (log scale; lower is better)');ax.set_title('JSON.'+op)
        ax.grid(axis='x',alpha=.15);ax.set_axisbelow(True)
        ax.spines[['top','right']].set_visible(False)
        if row==0 and col==0:ax.legend(loc='lower right')
fig.suptitle('Perry 0.5.1520 vs Node 26.5.1 vs Bun 1.3.14 — Apple M1',fontsize=15)
fig.supxlabel('* Parse returns a lazy array in Perry. Stringify always starts with fully materialized values.',fontsize=10)
fig.savefig(OUT/'cpu-comparison.png',dpi=160)
fig.savefig(OUT/'cpu-comparison.svg')
svg=OUT/'cpu-comparison.svg'
svg.write_text('\n'.join(line.rstrip() for line in svg.read_text().splitlines())+'\n')
print('Validated and summarized',len(summary),'groups; 150 output checks passed.')
