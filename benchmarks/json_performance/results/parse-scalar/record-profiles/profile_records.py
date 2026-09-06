from pathlib import Path
import subprocess,time,json
root=Path.cwd();out=root/'results/record-profiles';out.mkdir();rows=[]
for name,n in [('small_record',12000000),('records_object_1m',8000)]:
    command=[str(root/'.work/tape-worker'),str(root/'.work/fixtures'/(name+'.json')),'stringify',str(n),'2']
    with (out/(name+'.stdout')).open('w') as stdout,(out/(name+'.stderr')).open('w') as stderr:
        worker=subprocess.Popen(command,stdout=stdout,stderr=stderr)
        time.sleep(.3)
        sample=subprocess.run(['sample',str(worker.pid),'3','1','-file',str(out/(name+'.sample.txt'))],capture_output=True,text=True)
        rc=worker.wait(timeout=60)
    rows.append({'fixture':name,'command':command,'worker_exit':rc,'sample_exit':sample.returncode,'sample_stderr':sample.stderr})
    assert rc==0 and sample.returncode==0,rows[-1]
    print('PROFILED',name,flush=True)
(out/'metadata.json').write_text(json.dumps({'purpose':'Sampling only, separate from timed standings; preceding tape-scan worker','rows':rows},indent=2)+'\n')
