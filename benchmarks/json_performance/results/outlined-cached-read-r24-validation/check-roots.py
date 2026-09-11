from pathlib import Path
import hashlib,json,os,re,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent;main='--main' in sys.argv;arm='main' if main else 'candidate';build=w/('frozen-main' if main else 'frozen-build')
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};passes=subprocess.check_output(['python3',str(root/'scripts/read_statepoint_rewrite_passes.py')],cwd=root,text=True).strip()
shared=w.with_name('materialized-read-r23')
sources=[shared/(stem+'.ts') for stem in ['test_json_source_length','test_json_template_capture','test_json_cached_construction','test_json_cached_reads','prior-entry','callback-only','options-worker']]+[shared/'harness'/(stem+'.ts') for stem in ['worker','access-worker']];results=[]
for mode,rs4gc in [('native','1'),('shadow','0')]:
 d=w/(arm+'-ir-'+mode);d.mkdir(exist_ok=False)
 for source in sources:
  scratch=d/source.stem;scratch.mkdir();env=clean|{'PERRY_RUNTIME_DIR':str(build),'PERRY_WORKSPACE_ROOT':str(root),'PERRY_RS4GC':rs4gc,'PERRY_GC_MOVING_LOOP_POLLS':'1','PERRY_INLINE_SHADOW_SLOT':'0'}
  cmd=[str(build/'perry'),'compile',str(source),'--no-auto-optimize','--no-cache','--no-link','--trace','llvm','-o',str(scratch/'worker.o')]
  with (scratch/'compile.log').open('wb') as log:subprocess.run(cmd,cwd=scratch,env=env,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
  files=list((scratch/'.perry-trace/llvm').glob('*.ll'));assert files
  for i,source_ll in enumerate(files):
   dest=d/(source.stem+'-'+str(i)+'.ll')
   if mode=='native':
    with (scratch/('rewrite-'+str(i)+'.log')).open('wb') as log:subprocess.run(['/opt/homebrew/opt/llvm/bin/opt','-passes='+passes,'-S',str(source_ll),'-o',str(dest)],stdout=log,stderr=subprocess.STDOUT,check=True)
   else:dest.write_bytes(source_ll.read_bytes())
  print('IR',arm,mode,source.name,flush=True)
 files=list(d.glob('*.ll'));checker=['python3',str(root/'scripts/gc_root_dominance_check.py')]
 variants=[['--statepoints','--max-stale','0','--min-files','9','--min-statepoints','1','--min-live-bundles','1','--min-relocates','1']] if mode=='native' else [[],['--unrooted-allocas']]
 for index,flags in enumerate(variants):
  cmd=checker+flags+[str(p) for p in files]
  with (d/('check-'+str(index)+'.log')).open('wb') as log:r=subprocess.run(cmd,cwd=root,stdout=log,stderr=subprocess.STDOUT)
  results.append({'mode':mode,'scope':'all','variant':index,'exit_code':r.returncode,'command':cmd});print('ROOTS',arm,mode,index,r.returncode,flush=True)
 if mode=='native':
  for label,stems in [('ordinary-workers',['worker','access-worker']),('callback',['callback-only'])]:
   subset=[p for p in files if any(p.name.startswith(s+'-') for s in stems)];assert len(subset)==len(stems)
   cmd=checker+['--statepoints','--max-stale','0','--min-files',str(len(stems)),'--min-statepoints','1','--min-live-bundles','1','--min-relocates','1']+[str(p) for p in subset]
   with (d/(label+'-check.log')).open('wb') as log:r=subprocess.run(cmd,cwd=root,stdout=log,stderr=subprocess.STDOUT)
   results.append({'mode':mode,'scope':label,'exit_code':r.returncode,'command':cmd});print('ROOTS',arm,label,r.returncode,flush=True)
(w/(arm+'-roots.json')).write_text(json.dumps({'checks':results,'source_hashes':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},'compiler_sha256':hashlib.sha256((build/'perry').read_bytes()).hexdigest(),'runtime_sha256':hashlib.sha256((build/'libperry_runtime.a').read_bytes()).hexdigest(),'passes':passes},indent=2)+'\n')
# Existing fixture findings remain unsuppressed; compare actual baselines later.
