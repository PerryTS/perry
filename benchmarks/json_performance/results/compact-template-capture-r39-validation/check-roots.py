from pathlib import Path
import hashlib,json,os,re,shutil,subprocess,sys
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent;main='--main' in sys.argv;arm='main' if main else 'candidate';build=w/('frozen-main' if main else 'frozen-build')
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')};passes=subprocess.check_output(['python3',str(root/'scripts/read_statepoint_rewrite_passes.py')],cwd=root,text=True).strip()
shared=w.with_name('materialized-read-r23')
sources=[shared/(stem+'.ts') for stem in ['test_json_source_length','test_json_template_capture','test_json_cached_construction','test_json_cached_reads','prior-entry','callback-only','options-worker']]+[shared/'harness'/(stem+'.ts') for stem in ['worker','access-worker']];results=[]
for mode,rs4gc in [('native','1'),('shadow','0')]:
 d=w/(arm+'-ir-'+mode)
 if d.exists():
  assert mode=='native'
  proof=json.loads((w/'initial-root-reuse-refusal/proof.json').read_text())
  assert proof['source_commit']==subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
  assert all(hashlib.sha256((w/p).read_bytes()).hexdigest()==h for p,h in proof['native_files'].items())
  print('Reused already emitted current-source native IR with exact hashes',flush=True)
  continue
 d.mkdir(exist_ok=False)
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

exec((w/"run-fresh-root-checks.py").read_text())
