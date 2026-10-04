"""Read-only debugger attribution; no debugger RSS/time enters acceptance.
TS: arm the existing census at the third natural full mark. Zod is separately
built with one explicit diagnostic GC before safeParse, schema remains live.
"""
import hashlib,json,os,pathlib,struct,subprocess,time
E=pathlib.Path('/root/rss-gc-build-20261003/export-current0ed-v14'); O=E/'census-zod-v17';O.mkdir(exist_ok=True)
NODE='/root/rss-gc-build-20261003/node-v26.5.1-linux-x64/bin/node'
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def run(name,argv,env=None,timeout=1800):
 start=time.time()
 with (O/(name+'.out')).open('w') as out,(O/(name+'.err')).open('w') as err:
  r=subprocess.run(list(map(str,argv)),cwd=E,env=os.environ|(env or {}),stdout=out,stderr=err,timeout=timeout)
 record=dict(command=list(map(str,argv)),rc=r.returncode,seconds=time.time()-start,stdout_sha256=sha(O/(name+'.out')),stderr_sha256=sha(O/(name+'.err')))
 (O/(name+'.run.json')).write_text(json.dumps(record,indent=2)+'\n');assert r.returncode==0,record
 return record
oracle=json.loads((E/'gc-qb6-current0ed-v14-noauto-oracle.json').read_text()); records=[]
original=E/'sources/real/zodwork.ts';source=E/'sources/real/zodwork-census-v17.ts'
assert not source.exists()
text=original.read_text();needle='    const r = S.safeParse([';assert text.count(needle)==1
source.write_text('declare function gc(): void;\n'+text.replace(needle,'    if (i === 100) gc(); // diagnostic-only schema-live checkpoint\n'+needle))
node=run('node-diagnostic',[NODE,'--expose-gc',source,'200'],timeout=300)
assert (O/'node-diagnostic.out').read_text()==oracle['zodwork']
for arm in ['current0ed-base','current0ed-gc']:
 binary=E/'bin'/f'gc-qb6-current0ed-v17-diagnostic-{arm}-zodwork'
 env_build={'PERRY_RUNTIME_DIR':str(E/arm),'PERRY_WORKSPACE_ROOT':str(E.parent/('source-'+arm)),'PERRY_NO_CACHE':'1','PERRY_KEEP_SYMBOLS':'1','LLVM_SYS_221_PREFIX':'/usr/lib/llvm-22','CARGO_BUILD_JOBS':'8','CARGO_TARGET_DIR':str(E.parent/('target-'+arm)),'RAYON_NUM_THREADS':'8','CARGO_PROFILE_RELEASE_CODEGEN_UNITS':'16','CARGO_INCREMENTAL':'0'}
 build=run('build-'+arm,[E/arm/'perry','compile',source,'--no-auto-optimize','-o',binary],env_build,1800)
 build.update(source_sha256=sha(source),original_source_sha256=sha(original),compiler_sha256=sha(E/arm/'perry'),runtime_sha256=sha(E/arm/'libperry_runtime.a'),stdlib_sha256=sha(E/arm/'libperry_stdlib.a'),binary_sha256=sha(binary))
 (O/('build-'+arm+'-binding.json')).write_text(json.dumps(build,indent=2)+'\n')
 symbols=[s.split() for s in subprocess.check_output(['nm','-n',binary],text=True).splitlines()]
 def find(needle,suffix=False):
  hits=[s for s in symbols if len(s)==3 and (s[2].split('.llvm.')[0].endswith(needle) if suffix else needle in s[2])]
  assert len(hits)==1,(needle,hits);return int(hits[0][0],16),hits[0][2]
 arm_addr,arm_name=find('6census10census_arm',True);hook_addr,hook_name=find('6census21census_pass1_if_armed',True);_,take_name=find('6census11take_census',True)
 image=binary.read_bytes();phoff=struct.unpack_from('<Q',image,32)[0];phentsize,phnum=struct.unpack_from('<HH',image,54);label=None
 for i in range(phnum):
  typ,flags,off,va,pa,filesz,memsz,align=struct.unpack_from('<IIQQQQQQ',image,phoff+i*phentsize)
  if typ==1 and flags==4:
   pos=image.find(b'manual',off,off+filesz)
   if pos>=0:label=va+pos-off;break
 assert label is not None
 env={'META_ROOT':str(O),'META_TAKE_SYMBOL':take_name}
 for key,needle in [('SHAPE_DIR_SYMBOL','AGENT_SHAPE_DIR'),('SHAPE_EMPTY_PAGE_SYMBOL','10EMPTY_PAGE'),('SHAPE_EMPTY_CHUNK_SYMBOL','11EMPTY_CHUNK')]:env[key]=find(needle)[1]
 for rep in range(2):
  prefix=f'current0ed-v17-zodwork-{arm}-{rep}'; census=O/(prefix+'.jsonl');out=O/(prefix+'.app.out');err=O/(prefix+'.app.err');script=O/(prefix+'.gdb');assert not census.exists()
  script.write_text(f'''set pagination off
set confirm off
set language c
set disable-randomization on
set environment PERRY_GC_CENSUS {census}
set environment PERRY_GC_DIAG 1
set environment PERRY_GC_TRACE 1
set environment MIMALLOC_ALLOW_THP 0
source {E}/shape-storage-observer-v17.py
source {E}/current0ed-meta-observer-v17.py
run 200 > {out} 2> {err}
quit
''')
  r=run(prefix,['gdb','-q','--batch','-x',script,binary],env|{'META_PREFIX':prefix},300)
  assert out.read_text()==oracle['zodwork']
  rows=[json.loads(s) for s in census.read_text().splitlines()];assert len(rows)==1
  sharing=json.loads((O/(prefix+'.sharing.json')).read_text())
  for t in rows[0]['by_type']:
   assert sharing['live_counts'].get(str(t['id']),0)==t['live']['count'],t
   assert sharing['live_bytes'].get(str(t['id']),0)==t['live']['bytes'],t
  r.update(arm=arm,case='zodwork',repeat=rep,binary_sha256=sha(binary),source_sha256=sha(source),criterion='explicit diagnostic full before safeParse at iteration100, schema live; unmodified workload has no natural full; not benchmark RSS',oracle_correct=True,independent_live_counts_match=True)
  records.append(r);(O/'runs.json').write_text(json.dumps(records,indent=2)+'\n')
  print(prefix,sharing['ordinary_meta_bytes'],sharing['shapes']['records'],flush=True)
(O/'zod.exit').write_text('0\n')
