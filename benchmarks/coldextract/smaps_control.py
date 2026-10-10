from pathlib import Path
import os,subprocess,time,re,json,shutil
L=Path('/root/lanes/perry-coldextract');N='/root/lanes/11842/node-v24.9.0-linux-x64/bin/node';O=L/'rss';O.mkdir(exist_ok=True)
manifest={r['name']:r for r in json.loads((L/'bins-base/manifest.json').read_text())}
selection=json.loads((L/'summary/thp-selection.json').read_text());rows=[]
def descendants(pid):
 result=[pid];todo=[pid]
 while todo:
  p=todo.pop()
  try:kids=list(map(int,Path(f'/proc/{p}/task/{p}/children').read_text().split()))
  except OSError:continue
  result+=kids;todo+=kids
 return result
for kind,names in selection.items():
 for name in names:
  for thp_off in [False,True]:
   for arm in ['base','fix']:
    binary=str(L/('bins-'+arm)/('spool' if kind=='micro' and name=='spool' else 'micro' if kind=='micro' else 'upm' if kind=='upm' else name))
    args=['5'] if kind=='micro' and name=='spool' else [name,'1' if name.startswith('write') else '5'] if kind=='micro' else [] if kind=='upm' else manifest[name]['args']
    cmd=[binary,*args];env=os.environ.copy();env['OBSERVE']='0'
    if thp_off:env['MIMALLOC_ALLOW_THP']='0';cmd=[str(L/'tools/no-thp'),*cmd]
    tag=f'smaps-{kind}-{name}-{arm}-'+('thpoff' if thp_off else 'normal');stem=O/tag
    cwd=L/'micro' if kind=='micro' else L if kind=='upm' else Path(manifest[name]['cwd'])
    if kind=='upm':cmd=[str(L/'r.sh'),tag,name,'1',*cmd]
    peak=0;captures=0;huge=0;start=time.monotonic()
    with stem.with_suffix('.out').open('wb') as of,stem.with_suffix('.err').open('wb') as ef:
     p=subprocess.Popen(cmd,cwd=cwd,env=env,stdout=of,stderr=ef)
     try:
      while p.poll() is None:
       if time.monotonic()-start>330:raise TimeoutError(tag)
       for pid in descendants(p.pid):
        try:
         if os.readlink(f'/proc/{pid}/exe')!=binary:continue
         status=Path(f'/proc/{pid}/status').read_text()
         if thp_off:assert re.search(r'^THP_enabled:\s+0$',status,re.M)
         resident=int(re.search(r'^VmRSS:\s+(\d+)',status,re.M)[1])
         if resident<=peak+512:continue
         temp=stem.with_suffix('.latest.smaps')
         probe=subprocess.run([str(L/'tools/frozen-smaps'),str(pid),str(temp),binary],capture_output=True)
         if probe.returncode==75:continue
         assert probe.returncode==0,probe.stderr
         raw=temp.read_text();resident=sum(int(x) for x in re.findall(r'^Rss:\s+(\d+)',raw,re.M));anon_huge=sum(int(x) for x in re.findall(r'^AnonHugePages:\s+(\d+)',raw,re.M))
         if thp_off:assert anon_huge==0
         captures+=1
         if resident>peak:peak=resident;huge=anon_huge;stem.with_suffix('.peak.smaps').write_text(raw)
        except (FileNotFoundError,ProcessLookupError):pass
       time.sleep(.003)
     finally:
      if p.poll() is None:
       # Only this Popen child; no process-name or global kill.
       p.terminate();p.wait(timeout=10)
    assert p.returncode==0 and captures>0,tag
    if kind=='upm':
     result=(L/'runs'/tag/'res.tsv').read_text().split();assert result[0]=='0' and result[5]=='ok'
    elif kind=='programs':assert stem.with_suffix('.out').read_bytes()==(L/'bins-base'/(name+'.node.out')).read_bytes()
    if kind=='micro' and (name.startswith('write') or name=='spool'):
     for tree in (L/'work').glob('micro-*'):shutil.rmtree(tree)
    stem.with_suffix('.latest.smaps').unlink(missing_ok=True)
    row=dict(kind=kind,name=name,arm=arm,thp_off=thp_off,max_sampled_rss_kb=peak,anon_huge_pages_kb=huge,snapshots=captures,parity=True);rows.append(row);print(row,flush=True)
(L/'summary/smaps-controls.json').write_text(json.dumps(rows,indent=2))
