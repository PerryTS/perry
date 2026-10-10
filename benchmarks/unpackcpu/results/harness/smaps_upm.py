from pathlib import Path
import os,subprocess,time,shutil,json
L=Path('/root/lanes/perry-unpackcpu');rows=[]
for mode in ['cold','lock']:
 for n in range(5):
  for arm in (['base','fix'] if n%2==0 else ['fix','base']):
   W=L/'work'/f'smaps-upm-{mode}-{arm}-{n}';shutil.rmtree(W,ignore_errors=True);(W/'proj').mkdir(parents=True);(W/'store').mkdir()
   shutil.copyfile('/root/lanes/upm-prof/upm/bench/fixtures/nitro/package.json',W/'proj/package.json')
   e=os.environ.copy();e.update(UPM_STORE=str(W/'store'),UPM_REGISTRY='http://127.0.0.1:18771',MIMALLOC_ALLOW_THP='0',PERRY_GC_DIAG='1');e.pop('UPM_TRACE',None);e.pop('UPM_PHASES',None)
   cmd=[str(L/'tools/no-thp'),'taskset','-c','0-55','setarch','-R',str(L/'bins'/arm/'upm'),'install' if mode=='cold' else 'lock']
   snaps=[]
   with (W/'out').open('w') as out,(W/'err').open('w') as err:
    child=subprocess.Popen(cmd,cwd=W/'proj',env=e,stdout=out,stderr=err)
    while child.poll() is None:
     try:
      if os.readlink(f'/proc/{child.pid}/exe')==str(L/'bins'/arm/'upm'):
       text=Path(f'/proc/{child.pid}/smaps_rollup').read_text();status=Path(f'/proc/{child.pid}/status').read_text()
       snap={k:int(next(line.split()[1] for line in text.splitlines() if line.startswith(k+':'))) for k in ['Rss','Anonymous','Private_Clean','Private_Dirty','Shared_Clean','Shared_Dirty','AnonHugePages']}
       assert snap['AnonHugePages']==0 and int(next(line.split()[1] for line in status.splitlines() if line.startswith('THP_enabled:')))==0
       snaps.append(snap)
     except (FileNotFoundError,ProcessLookupError,StopIteration):pass
     time.sleep(.005)
   assert child.returncode==0 and snaps
   row=dict(mode=mode,arm=arm,n=n,peak=max(snaps,key=lambda x:x['Rss']),fulls=sum(line.startswith('[gc-full] site=') for line in (W/'err').read_text().splitlines()),diagnostic_only=True)
   rows.append(row);(L/'evidence/smaps-upm.json').write_text(json.dumps(rows,indent=2)+'\n');shutil.rmtree(W)
from statistics import median
for mode in ['cold','lock']:
 for arm in ['base','fix']:
  rs=[r for r in rows if r['mode']==mode and r['arm']==arm];print(mode,arm,{k:median(r['peak'][k] for r in rs) for k in ['Rss','Anonymous','Private_Clean','Private_Dirty','Shared_Clean']},'fulls',[r['fulls'] for r in rs],flush=True)
print('COMPLETE',flush=True)
