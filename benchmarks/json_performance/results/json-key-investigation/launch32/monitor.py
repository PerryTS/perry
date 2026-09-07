from pathlib import Path
import subprocess,time,json
root=Path(__file__).resolve().parent
allowed=['json-scalar-runtime-v31-20260907-codex/','json-launch-control-v25-20260907-codex/','json-structural-runtime-v24-20260907-codex/','json-depth-runtime-v22-20260907-codex/','json-fused-utf16-v11-20260907-codex/','json-escape-runtime-v18-20260907-codex/','json-scalar-immutable-v32-20260907-codex/','json-shared-admission-v5-20260907-codex/','json-deferred-gc-v2-20260907-codex/']
p=subprocess.Popen(['python3',str(root/'with_lock.py'),'--','python3',str(root/'all.py')])
observations=[]
while p.poll() is None:
 ps=subprocess.run(['ps','-Ao','pid,ppid,pcpu,comm'],capture_output=True,text=True)
 external=[line.strip() for line in ps.stdout.splitlines() if any(x in line for x in ['/ccperf/','/rustc','/cargo','/worker','/active']) and not any(x in line for x in allowed)]
 top=sorted(ps.stdout.splitlines()[1:],key=lambda line:float(line.split()[2]),reverse=True)[:8]
 observations.append({'load':__import__('os').getloadavg(),'top_cpu':top,'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'monitor_exit':ps.returncode,'external':external,'xprotect_busy':[line.strip() for line in ps.stdout.splitlines()[1:] if 'xprotect' in line.lower() and float(line.split()[2])>5.0]})
 tmp=root/'results/trace-observations.tmp';tmp.write_text(json.dumps(observations,indent=2)+'\n');tmp.replace(root/'results/trace-observations.json')
 time.sleep(5)
print('MONITOR_FINISHED',p.returncode,'observations',len(observations),flush=True)
assert p.returncode==0
assert all(r['monitor_exit']==0 and not r['external'] and not r['xprotect_busy'] for r in observations)

assert json.loads((root/'results/window-custom.json').read_text())['quiet_gate_passed']
