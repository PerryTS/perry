from pathlib import Path
import hashlib,json,os,subprocess,re
root=Path('/Users/amlug/projects/perry/json-merged-pr10022')
os.chdir(root)
work=root/'benchmarks/json_performance/.work/tape-depth-r3'
provenance=json.loads((work/'provenance.json').read_text())
for p in ['target/release/perry','target/release/libperry_runtime.a','target/release/libperry_stdlib.a']:
    assert hashlib.sha256(Path(p).read_bytes()).hexdigest()==provenance['files'][p]
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
source=root/'test-files/test_json_tape_depth_admission.ts'
binary=work/'depth-admission'
compile_cmd=[str(root/'target/release/perry'),'compile',str(source),'--no-cache','-o',str(binary)]
with (work/'depth-compile.log').open('wb') as log:
    subprocess.run(compile_cmd,env=clean|{'PERRY_RUNTIME_DIR':str(root/'target/release'),'PERRY_NO_AUTO_OPTIMIZE':'1'},stdout=log,stderr=subprocess.STDOUT,check=True,timeout=180)
node=['/opt/homebrew/bin/node','--experimental-strip-types',str(source)]
expected=subprocess.check_output(node,env=clean,timeout=90)
(work/'depth-node.stdout').write_bytes(expected)
stress={'PERRY_GC_SCHEDULE_SEED':'10022','PERRY_GC_SCHEDULE_RATE':'0.1','PERRY_GC_SCHEDULE_ALLOC_KB':'0','PERRY_GC_PROTECT_FROMSPACE':'1'}
matrix=[]
for mode,mode_env in [('auto',{}),('tape',{'PERRY_JSON_TAPE':'1'}),('direct',{'PERRY_JSON_TAPE':'0'})]:
    for gc,gc_env in [('normal',{}),('scheduled',stress),('fullgc',{'PERRY_GEN_GC':'0'})]:
        label='depth-'+mode+'-'+gc
        with (work/(label+'.stdout')).open('wb') as out,(work/(label+'.stderr')).open('wb') as err:
            subprocess.run([str(binary)],env=clean|mode_env|gc_env|{'PERRY_GC_DIAG':'1'},stdout=out,stderr=err,check=True,timeout=180)
        assert (work/(label+'.stdout')).read_bytes()==expected,label
        diagnostic=(work/(label+'.stderr')).read_text()
        protected=len(re.findall(r'\[gc-fromspace-protect\].*retired_set=#',diagnostic))
        moved=sum(sum(map(int,re.findall(r'\b(?:copied_objects|promoted_objects)=(\d+)',line))) for line in diagnostic.splitlines() if line.startswith('[gc-copy-minor] ran'))
        if gc=='scheduled':assert protected>0 and moved>0,(label,protected,moved)
        matrix.append({'mode':mode,'gc':gc,'matches_node':True,'protected_retired_sets':protected,'moved_objects':moved})
        print('PASS',label,protected,moved,flush=True)
(work/'compiled-depth-matrix.json').write_text(json.dumps({'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'worker_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'compile_command':compile_cmd,'matrix':matrix},indent=2)+'\n')
