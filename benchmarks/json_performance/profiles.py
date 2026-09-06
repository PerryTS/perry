#!/usr/bin/env python3
"""Sampling diagnostics, run separately from measured comparisons."""
from pathlib import Path
import os, signal, subprocess, time
root=Path(__file__).resolve().parent
out=root/'results/profiles';out.mkdir(exist_ok=True)
for name,op,n in [('tiny_object','stringify',50000000),('long_string_1m','parse',15000),('long_string_1m','stringify',15000),('wide_1m','parse',200),('records_array_16k','scan',3724),('numbers_1m','stringify',2000)]:
    label=name+'-'+op
    with (out/(label+'.stdout')).open('w') as stdout, (out/(label+'.stderr')).open('w') as stderr:
        proc=subprocess.Popen([str(root/'.work/profile_worker'),str(root/'.work/fixtures'/(name+'.json')),op,str(n),'2'],stdout=stdout,stderr=stderr,start_new_session=True)
        time.sleep(1)
        sample=subprocess.run(['/usr/bin/sample',str(proc.pid),'4','1','-mayDie','-file',str(out/(label+'.txt'))],capture_output=True,text=True,timeout=15)
        print('PROFILE',label,sample.returncode,flush=True)
        if proc.poll() is None:os.killpg(proc.pid,signal.SIGTERM)
        try:proc.wait(timeout=5)
        except subprocess.TimeoutExpired:os.killpg(proc.pid,signal.SIGKILL);proc.wait()
