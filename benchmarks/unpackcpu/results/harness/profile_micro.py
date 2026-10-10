from pathlib import Path
import subprocess,sys,os,fcntl,json,time,shutil
L=Path('/root/lanes/perry-unpackcpu');O=L/'profiles';O.mkdir(exist_ok=True);N='/root/lanes/11842/node-v24.9.0-linux-x64/bin/node'
arm=sys.argv[1];lock=open('/root/MEASURE.lock','w');fcntl.flock(lock,fcntl.LOCK_EX);Path('/root/MEASURE.holder').write_text('perry-unpackcpu/profile-'+arm+'\n')
metadata=[]
for mode,sel,rounds in [('worker','large',10),('worker-stream','large',10),('worker-stream','all',5),('inflate','large',20),('parse','all',10),('hash','all',20),('sha256','all',100),('sha1','all',100)]:
 stem=O/f'{arm}-{mode}-{sel}';cmd=[str(L/'bins'/arm/'micro')] if arm!='node' else [N,'--perf-basic-prof','--experimental-strip-types',str(L/'micro/micro.ts')]
 with Path(str(stem)+'.out').open('w') as out,Path(str(stem)+'.err').open('w') as err:
  r=subprocess.run(['taskset','-c','56-63','setarch','-R','perf','record','-q','-e','cycles:u','-F','999','--call-graph','dwarf,8192','-o',str(stem)+'.data','--',str(L/'tools/time-exact'),str(stem)+'.time',*cmd,mode,sel,str(rounds),str(L/'work/profile-store')],stdout=out,stderr=err,timeout=150)
 assert r.returncode==0
 with Path(str(stem)+'.report').open('w') as f:subprocess.run(['perf','report','--stdio','-i',str(stem)+'.data','--no-children','--sort','comm,dso,symbol','--percent-limit','0.1'],stdout=f,stderr=subprocess.DEVNULL,check=True)
 with Path(str(stem)+'.samples').open('w') as f:subprocess.run(['perf','script','-i',str(stem)+'.data','-F','comm,tid,period,ip,sym,dso'],stdout=f,stderr=subprocess.DEVNULL,check=True)
 with Path(str(stem)+'.leaves').open('w') as f:subprocess.run(['perf','script','-G','-i',str(stem)+'.data','-F','comm,tid,period,ip,sym,dso'],stdout=f,stderr=subprocess.DEVNULL,check=True)
 Path(str(stem)+'.data').unlink();shutil.rmtree(L/'work/profile-store',ignore_errors=True)
 wall,user,sy,rss=map(float,Path(str(stem)+'.time').read_text().split())
 metadata.append(dict(arm=arm,mode=mode,selection=sel,rounds=rounds,wall=wall,user=user,sys=sy,rss_kb=int(rss),cpus='56-63'))
 (O/('metadata-'+arm+'.json')).write_text(json.dumps(metadata,indent=2))
 print(arm,mode,sel,'COMPLETE',flush=True)
# Runs after the micro profiles, under the same CPU 56-63 lock.
stem = O / f'{arm}-upm-cold'
prefix = ['taskset', '-c', '56-63', 'setarch', '-R', 'perf', 'record', '-q',
          '-e', 'cycles:u', '-F', '999', '--call-graph', 'dwarf,8192',
          '-o', str(stem) + '.data', '--']
env = os.environ.copy()
env.update(OBSERVE='0', XENV='UPM_REGISTRY=http://127.0.0.1:18771', PRE=' '.join(prefix))
cmd = [str(L / 'bins' / arm / 'upm')] if arm != 'node' else [N, '--perf-basic-prof', str(L / 'node-upm/dist/upm.mjs')]
tag = 'profile-upm-' + arm
with Path(str(stem) + '.runner.log').open('w') as log:
    result = subprocess.run([str(L / 'r.sh'), tag, 'cold', '1', *cmd],
                            env=env, stdout=log, stderr=log, timeout=180)
assert result.returncode == 0
fields = (L / 'runs' / tag / 'res.tsv').read_text().split()
assert fields[0] == '0' and fields[-1] == 'ok'
wall, user, sy, rss = map(float, fields[1:5])
with Path(str(stem) + '.report').open('w') as report:
    subprocess.run(['perf', 'report', '--stdio', '-i', str(stem) + '.data', '--no-children',
                    '--sort', 'comm,dso,symbol', '--percent-limit', '0.1'],
                   stdout=report, stderr=subprocess.DEVNULL, check=True)
with Path(str(stem) + '.samples').open('w') as samples:
    subprocess.run(['perf', 'script', '-i', str(stem) + '.data',
                    '-F', 'comm,tid,period,ip,sym,dso'],
                   stdout=samples, stderr=subprocess.DEVNULL, check=True)
with Path(str(stem) + '.leaves').open('w') as leaves:
    subprocess.run(['perf', 'script', '-G', '-i', str(stem) + '.data',
                    '-F', 'comm,tid,period,ip,sym,dso'],
                   stdout=leaves, stderr=subprocess.DEVNULL, check=True)
Path(str(stem) + '.data').unlink()
metadata.append(dict(arm=arm, mode='upm-cold', selection='all', rounds=1,
                     wall=wall, user=user, sys=sy, rss_kb=int(rss), cpus='56-63'))
(O / ('metadata-' + arm + '.json')).write_text(json.dumps(metadata, indent=2))
print(arm, 'upm-cold', 'COMPLETE', flush=True)

print('COMPLETE',arm,flush=True)
