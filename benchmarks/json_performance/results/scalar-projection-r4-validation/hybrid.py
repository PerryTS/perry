from pathlib import Path
import hashlib, json, os, subprocess
root=Path(__file__).resolve().parents[4]
work=Path(__file__).resolve().parent
main=work.parent/'main-53df-fresh'
r3=work.parent/'scalar-projection-r3'
obj=main/'worker.o'
lib=r3/'frozen-build/libperry_runtime.a'
main_meta=json.loads((main/'provenance.json').read_text())
r3_meta=json.loads((r3/'build-provenance.json').read_text())
assert hashlib.sha256(obj.read_bytes()).hexdigest()==main_meta['files'][str(obj.relative_to(root))]
assert hashlib.sha256(lib.read_bytes()).hexdigest()==r3_meta['artifacts']['libperry_runtime.a']
binary=work/'main-codegen-r3-runtime-worker'
cmd=['cc',str(obj),str(lib),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)]
with (work/'hybrid-link.log').open('wb') as out:subprocess.run(cmd,stdout=out,stderr=subprocess.STDOUT,check=True)
# Existing main imports have unchanged ABI; scalar projection only adds a helper.
# The tiny diagnostic uses an eager object, with unchanged Object/Array layouts.
source=root/'benchmarks/json_performance/worker.js'
fixture=root/'benchmarks/json_performance/.work/fixtures/small_record.json'
args=[str(fixture),'stringify','1000','8','verify']
clean={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
node=subprocess.check_output(['/opt/homebrew/bin/node',str(source)]+args,env=clean,text=True)
def observable(s):
 lines=s.splitlines()
 result=next(l for l in lines if l.startswith('RESULT ')).split()
 return result[6:]+[l for l in lines if l.startswith(('VERIFY ','KEEP '))]
checks=[]
for name,knobs in [('normal',{}),('scheduled',{'PERRY_GC_SCHEDULE_SEED':'10022','PERRY_GC_SCHEDULE_RATE':'0.1','PERRY_GC_SCHEDULE_ALLOC_KB':'0','PERRY_GC_PROTECT_FROMSPACE':'1','PERRY_GC_DIAG':'1'}),('fullgc',{'PERRY_GEN_GC':'0'})]:
 r=subprocess.run([str(binary)]+args,env=clean|knobs,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,check=True)
 (work/('hybrid-'+name+'.stdout')).write_text(r.stdout);(work/('hybrid-'+name+'.stderr')).write_text(r.stderr)
 assert observable(r.stdout)==observable(node),(name,r.stdout,node)
 checks.append({'mode':name,'matches_node':True})
 print('HYBRID PASS',name,flush=True)
(work/'hybrid-node.stdout').write_text(node)
meta={'note':'Diagnostic crossover only: actual main worker object linked to frozen R3 runtime. Not an R4 product binary. Existing imported function ABI and Object/Array layouts unchanged. Tiny stringify eager-object case only.','main_source_commit':main_meta['source_commit'],'runtime_source_commit':r3_meta['source_commit'],'commands':[cmd],'files':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [obj,lib,binary,fixture,source]},'checks':checks}
(work/'hybrid-provenance.json').write_text(json.dumps(meta,indent=2)+'\n')
