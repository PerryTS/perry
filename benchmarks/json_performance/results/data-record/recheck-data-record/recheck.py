from pathlib import Path
import importlib.util,json,random,sys,time,hashlib,subprocess,os
root=Path.cwd()
sys.argv=['run.py']
spec=importlib.util.spec_from_file_location('runner',root/'run.py');r=importlib.util.module_from_spec(spec);spec.loader.exec_module(r)
manifest={f['name']:f for f in r.FIXTURES}
rows=[json.loads(s) for s in (root/'results/fastpaths/timing.jsonl').read_text().splitlines()]
iters={(x['fixture'],x['operation']):x['iterations'] for x in rows}
cases=[('unicode',f,o) for f,o in [
 ('numbers_1m','parse'),('heterogeneous_1m','parse'),('escaped_1m','stringify'),
 ('long_string_1m','parse'),('heterogeneous_1m','stringify'),('wide_1m','stringify')]]
cases += [('tape',f,o) for f,o in [
 ('empty_object','parse'),('tiny_object','parse'),('small_record','parse'),('object_1k','parse'),
 ('empty_object','stringify'),('tiny_object','stringify'),('small_record','stringify'),
 ('records_array_16k','stringify'),('records_object_1m','stringify'),
 ('records_object_8m','stringify'),('records_object_20m','stringify'),('numbers_1m','stringify')]]
out=root/'results/recheck-data-record';out.mkdir()
meta={'time_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'load_before':os.getloadavg(),'cases':cases,'repetitions':7,'notes':'Same worker entry, default GC, fresh processes. Call counts are fixed from the full matrix. Randomized two-arm order per repetition. CPU includes user and system time.'}
meta['worker_sha256']={n:hashlib.sha256((root/'.work'/p).read_bytes()).hexdigest() for n,p in [('unicode','baseline-worker'),('tape','tape-worker'),('candidate','worker')]}
(out/'metadata.json').write_text(json.dumps(meta,indent=2)+'\n')
rng=random.Random(24019)
for reference,name,op in cases:
    f=manifest[name];n=iters[name,op];warm=5000 if f['bytes']<4096 else 2
    r.ENGINES={'baseline':[str(root/'.work'/{'unicode':'baseline-worker','tape':'tape-worker'}[reference])],'perry':[str(root/'.work/worker')]}
    dest=out/(reference+'-'+name+'-'+op+'.jsonl')
    for rep in range(7):
        order=list(r.ENGINES);rng.shuffle(order)
        for engine in order:
            row=r.one(engine,f,op,n,warm);row.update(rep=rep,reference=reference,bytes=f['bytes'])
            with dest.open('a') as file:file.write(json.dumps(row,separators=(',',':'))+'\n')
            assert 'error' not in row,row
    print('CHECKED',reference,name,op,n,flush=True)
