from pathlib import Path
import subprocess,os,time,fcntl
L=Path('/root/lanes/perry-unpackcpu')
while not (L/'compile-fix.rc').exists():time.sleep(15)
assert (L/'compile-fix.rc').read_text().strip()=='0'
e=os.environ.copy();e['ARMS']='base,fix,node';e.pop('PERRY_NO_AUTO_OPTIMIZE',None)
programs=['hello','tsc','zod','qs_parse','qs_stringify','commander','fastify','effect','buffer_heavy','worker_heavy']
def run(tag,cmd,env=e):
 with (L/(tag+'.log')).open('w') as f:r=subprocess.run(['taskset','-c','0-55',*cmd],env=env,stdout=f,stderr=f)
 (L/(tag+'.rc')).write_text(str(r.returncode));print(tag,r.returncode,flush=True);assert r.returncode==0
run('micro-inst',['python3',str(L/'micro_measure.py'),'inst'])
run('upm-instructions',['python3',str(L/'measure.py'),'upm-instructions','cold','lock','offinst'])
run('programs-instructions',['python3',str(L/'measure.py'),'programs-instructions',*programs])
run('micro-gc',['python3',str(L/'micro_measure.py'),'gc'])
run('upm-gc',['python3',str(L/'measure.py'),'upm-gc','cold','lock','offinst'])
run('programs-gc',['python3',str(L/'measure.py'),'programs-gc',*programs])
run('provider-final',['python3',str(L/'provider_final.py')])
run('micro-wall',['python3',str(L/'micro_measure.py'),'wall'])
with open('/root/MEASURE.lock','w') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX);run('upm-wall',['python3',str(L/'measure.py'),'upm-wall','cold','lock'])
# Replay both baseline and changed symbols on the corrected identical driver.
for arm in ['base','fix','node']:run('profile-final-'+arm,['python3',str(L/'profile_micro.py'),arm])
print('COMPLETE',flush=True)
