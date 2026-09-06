#!/usr/bin/env python3
"""Targeted decomposition; these results are not default-runtime standings."""
import json
import run as bench
def row(i):
    return dict(id=i,name='user_'+str(i),email='user_'+str(i)+'@example.com',active=i%3!=0,score=i*1.5,tags=['tag_'+str(i%10),'tag_'+str(i%7)])
root=bench.ROOT
# Snapshot any additional fixture definitions separately from the primary corpus.
def fixture(name,value):
    blob=json.dumps(value,ensure_ascii=False,separators=(',',':')).encode()
    (root/'.work/fixtures'/(name+'.json')).write_bytes(blob)
    return {'name':name,'bytes':len(blob)}

def record(kind,result):
    result['diagnostic']=kind
    bench.save('diagnostics',result)
    print(kind,result['fixture'],result['operation'],result['engine'],result['iterations'],result.get('tape'),result.get('cpu_ms'),result.get('peak_rss'),result.get('error',''),flush=True)

f=next(f for f in bench.FIXTURES if f['name']=='records_array_16k')
for n in [10,100,500,1000]:
    for tape in [None,0]:
        record('scan_scaling',bench.one('perry',f,'scan',n,2,tape=tape))
for count in [64,128,129,256]:
    f=fixture('diagnostic_rows_'+str(count),[row(i) for i in range(count)])
    for tape in [None,0]:
        record('scan_threshold',bench.one('perry',f,'scan',500,2,tape=tape))
for count in [1000,5000,10000,25000]:
    f=fixture('diagnostic_wide_'+str(count),{'field_'+str(i):i for i in range(count)})
    for e in ['perry','node','bun']:
        record('wide_scaling',bench.one(e,f,'parse',5,2))
for name in ['small_record','records_object_1m']:
    f=next(f for f in bench.FIXTURES if f['name']==name)
    for e in ['perry','node','bun']:
        record('pretty',bench.one(e,f,'pretty',1000 if name=='small_record' else 10,2,verify=True))
