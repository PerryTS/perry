from pathlib import Path
import hashlib,json,os,re,shutil,subprocess,sys
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
 # A runtime-only change must emit the same IR; re-use the already checked
 # R39 fresh verdict only after proving all emitted IR and checker sources unchanged.
 assert not main, 'The R26 reference verdicts were already archived.'
 previous=w/('main-ir-'+mode)
 files=sorted(d.glob('*.ll'));expected=sorted(previous.glob('*.ll'))
 assert [p.name for p in files]==[p.name for p in expected] and len(files)==9
 for current,old in zip(files,expected):
  a=current.read_bytes();b=old.read_bytes()
  if mode=='native':
   assert a.startswith(b'; ModuleID = ') and b.startswith(b'; ModuleID = ')
   a=a.split(b'\n',1)[1];b=b.split(b'\n',1)[1]
  assert a==b,(mode,current.name,'IR changed; a fresh checker run is required')
 assert not subprocess.check_output(['git','diff',json.loads((w/'main-roots.json').read_text())['checker_source_commit'],'HEAD','--','scripts'],cwd=root)
 record=json.loads((w/'main-roots.json').read_text())
 assert record['checker_sha256']==hashlib.sha256((root/'scripts/gc_root_dominance_check.py').read_bytes()).hexdigest()
 assert passes==record['passes']
 for path,digest in record['source_hashes'].items():assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
 for item in record['checks']:
  if item['mode']!=mode:continue
  name='check-'+str(item['variant'])+'.log' if item['scope']=='all' else item['scope']+'-check.log'
  assert hashlib.sha256((previous/name).read_bytes()).hexdigest()==item['log_sha256'],name
 for old in previous.glob('*check*.log'):shutil.copy2(old,d/old.name)
 results.extend(r for r in json.loads((w/'main-roots.json').read_text())['checks'] if r['mode']==mode)
 print('ROOTS',arm,mode,'R39 fresh verdict for R26 IR reused after exact IR and checker-source equivalence',flush=True)
(w/(arm+'-roots.json')).write_text(json.dumps({'checks':results,'source_hashes':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},'compiler_sha256':hashlib.sha256((build/'perry').read_bytes()).hexdigest(),'runtime_sha256':hashlib.sha256((build/'libperry_runtime.a').read_bytes()).hexdigest(),'passes':passes,'verdict_reuse':{'source_commit':json.loads((w/'main-roots.json').read_text())['checker_source_commit'],'note':'Fresh candidate IR matches the R26 reference IR checked freshly at R39. The entire scripts tree is unchanged since that checker source commit. All recorded commands and copied logs describe those actual prior executions, not new checker runs. Preserve every unsuppressed finding.'}},indent=2)+'\n')
# Existing fixture findings remain unsuppressed; compare actual baselines later.
