#!/usr/bin/env python3
"""Build no compilers: compile lane workloads and collect interleaved evidence."""
import argparse, hashlib, json, os, re, subprocess, shutil, time
from pathlib import Path
ROOT = Path(os.environ.get('KEYLISTS_ROOT', '/root/codex-lanes/cx-keylists'))
EVIDENCE = ROOT / os.environ.get('KEYLISTS_EVIDENCE', 'evidence')
MAIN = ROOT / os.environ.get('KEYLISTS_MAIN', 'main')
CPUS = os.environ.get('KEYLISTS_CPUS', '0-23,32-55')
PROGRAMS = {
 'hello': ('', 'hello.ts', []),
 'tscwork': ('', 'tscwork.ts', ['1']),
 'zodwork': ('', 'zodwork.ts', ['5000']),
 'qs_parse': ('pk', 'qs/parse_nested.ts', []),
 'qs_stringify': ('pk', 'qs/stringify_nested.ts', []),
 'commander': ('pk', 'commander/parse_argv.ts', []),
 'fastify': ('pk', 'fastify/inject.ts', ['5000','300']),
 'effect': ('effect', 'main.ts', []),
 'property-prototype': ('wit', 'property-prototype.ts', []),
 'shapes-options': ('wit', 'shapes-options.ts', []),
 'schema-record': ('wit', 'schema-record.ts', []),
 'root-survival': ('wit', 'root-survival.ts', []),
 'buffer_heavy': ('', 'buffer_heavy.ts', []),
 'worker_heavy': ('', 'worker_heavy.ts', []),
}
for micro in ['class_getter', 'object_array', 'class_virtual', 'object_spread']:
 PROGRAMS[micro] = ('micros', micro+'.ts', ['200000'])
PROGRAMS['tscwork'] = PROGRAMS.pop('tscwork')
NODE = ['taskset','-c','0-23,32-55','node', '--disable-warning=MODULE_TYPELESS_PACKAGE_JSON', '--experimental-strip-types']
def arm_env(arm):
 source_arm=arm.removeprefix('early-')
 base = ROOT if source_arm=='head' else (MAIN if source_arm=='main' else ROOT/source_arm)
 env = dict(os.environ, CARGO_TARGET_DIR=str(base/'target'), PERRY_RUNTIME_DIR=str(base/'target/release'), PERRY_WORKSPACE_ROOT=str(base/'src'), RUST_TEST_THREADS='1', CARGO_BUILD_JOBS=os.environ.get('CARGO_BUILD_JOBS','8'), PERRY_CODEGEN_UNIT_JOBS=os.environ.get('KEYLISTS_CODEGEN_JOBS','8'), PERRY_ALLOW_PERRY_FEATURES='1', PERRY_NO_AUTO_OPTIMIZE='1', PERRY_NO_CACHE='1')
 env.pop('PERRY_GC_DIAG', None)
 if arm.startswith(('obs-', 'phase-')):
  original=ROOT if arm in ('obs-head','phase-head') else MAIN
  env['KEYLISTS_ORIGINAL_BASE']=str(original)
  env['KEYLISTS_OBS_BASE']=str(base)
 return base, env

def normalized(data):
 return b'\n'.join(l for l in data.splitlines() if not re.match(rb'^(ms[ :]|elapsed_ms[ :])',l))

def run(cmd, cwd, env, dest, timeout=1800):
 result = subprocess.run(cmd,cwd=cwd,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=timeout)
 dest.with_suffix(dest.suffix+'.out').write_bytes(result.stdout)
 dest.with_suffix(dest.suffix+'.err').write_bytes(result.stderr)
 return result

def setup_shim(arm):
 base, env = arm_env(arm)
 out = EVIDENCE/arm
 out.mkdir(parents=True,exist_ok=True)
 archive = base/'target/release/libperry_runtime.a'
 symbols = subprocess.check_output(['nm','--defined-only',str(archive)],stderr=subprocess.DEVNULL).decode()
 def symbol(name):
  found = {l.split()[-1] for l in symbols.splitlines() if l.split() and l.split()[-1].endswith(name)}
  assert len(found)==1,(name,found)
  return found.pop()
 names={'obs_minors':'COPYING_MINORS','obs_step':'STEP_TOTAL_US','obs_remark':'REMARK_TOTAL_US','obs_minor':'MINOR_TOTAL_US','obs_full':'FULL_TOTAL_US'}
 names={alias:symbol(name) for alias,name in names.items()}
 libdir=out/'observe-runtime'
 libdir.mkdir(exist_ok=True)
 for library in (base/'target/release').glob('*.a'):
  dest=libdir/library.name
  if dest.exists() or dest.is_symlink():dest.unlink()
  if library.name=='libperry_runtime.a':shutil.copy2(library,dest)
  else:dest.symlink_to(library)
 subprocess.run(['objcopy',*[f'--globalize-symbol={name}' for name in names.values()],str(libdir/'libperry_runtime.a')],check=True)
 header = out/'gc-symbols.h'
 total_names=['PERRY_LANE_FULL_COUNT','PERRY_LANE_MINOR_COUNT','PERRY_LANE_FULL_PAUSE_US','PERRY_LANE_MINOR_PAUSE_US']
 phase_names=['PERRY_LANE_PHASE_INS', 'PERRY_LANE_PHASE_US', 'PERRY_LANE_KEY_BIRTHS', 'PERRY_LANE_KEY_BYTES']
 if all(name in symbols for name in phase_names): total_names += phase_names
 totals=''
 if all(name in symbols for name in total_names):
  totals='#define KEYLISTS_PROCESS_TOTALS 1\n'+'\n'.join('extern _Atomic uint64_t '+name+('[10]' if 'PHASE_' in name else '')+';' for name in total_names)+'\n'
 header.write_text('\n'.join(f'extern _Atomic uint64_t {alias} __asm__("{name}");' for alias,name in names.items())+'\n'+totals)
 subprocess.run(['cc','-O2','-I',str(out),'-c',str(ROOT/'src/lane-keylists/gc-observe.c'),'-o',str(out/'gc-observe.o')],check=True)
 env['KEYLISTS_COUNTER_LINK_ARGS']=' '.join('-Wl,--undefined='+name for name in names.values())
 return out, env

def compile_arm(arm, only=None):
 out, env=setup_shim(arm)
 base,_=arm_env(arm)
 statuses={}
 for name,(directory,source,args) in PROGRAMS.items():
  if only and name not in only:continue
  if arm=='main' and name=='tscwork' and not os.environ.get('KEYLISTS_EARLY_TSC'):
   while (ROOT/'tmp/early-tsc.running').exists():time.sleep(10)
   proof=ROOT/'tmp/early-tsc.json'
   if proof.exists():
    saved=json.loads(proof.read_text())
    assert saved['compiler']==hashlib.sha256((base/'target/release/perry').read_bytes()).hexdigest()
    assert all(hashlib.sha256(Path(path).read_bytes()).hexdigest()==digest for path,digest in saved['inputs'].items())
    assert saved['status']['equal'] and saved['status']['compile_rc']==saved['status']['node_rc']==saved['status']['run_rc']==0
    assert (out/(name+'.link.json')).exists()
    statuses[name]=saved['status']
    print(arm,name,'reused verified fresh overlap frontend',flush=True)
    continue
  cwd=ROOT/'drivers'/directory
  local=dict(env)
  local['PERRY_MODULE_JOBS']=os.environ.get('KEYLISTS_MODULE_JOBS','8')
  reserved=2*sum((ROOT/'tmp'/flag).exists() for flag in ['early-tsc.running','native-current.running','runtime-current.running'])
  local['PERRY_MODULE_JOBS']=str(min(8-reserved,int(local['PERRY_MODULE_JOBS'])))
  local['PERRY_CODEGEN_UNIT_JOBS']='1'
  if name=='tscwork':
   local['PERRY_LL_RS4GC_MAX_INSTRS']='2097152'
   local['PERRY_MODULE_JOBS']=os.environ.get('KEYLISTS_MODULE_JOBS','8')
   local['PERRY_CODEGEN_UNIT_JOBS']='1'
  for observed in [True]:
   label=name+('.observed' if observed else '')
   if observed:
    local['PERRY_RUNTIME_DIR']=str(out/'observe-runtime')
    local['PERRY_EXTRA_LINK_ARGS']=str(out/'gc-observe.o')+' -Wl,--wrap=js_gc_release_current_thread_collection_side_allocations '+env.get('KEYLISTS_COUNTER_LINK_ARGS','')
    local['KEYLISTS_PURE_OUTPUT']=str(out/name)
    local['KEYLISTS_OBSERVER_OBJECT']=str(out/'gc-observe.o')
    local['PATH']=str(ROOT/'src/lane-keylists')+':'+local['PATH']
   else:
    local['PERRY_RUNTIME_DIR']=env['PERRY_RUNTIME_DIR']
    local.pop('PERRY_EXTRA_LINK_ARGS',None)
   result=run(['taskset','-c',CPUS,str(base/'target/release/perry'),'compile','--no-auto-optimize','--cache-dir',str(out/'cache'/label),source,'-o',str(out/label)],cwd,local,out/(label+'.compile'),timeout=7200)
   if result.returncode:
    statuses[name]={'compile_rc':result.returncode,'observed':observed}
    print(arm,name,'compile FAIL',result.returncode,flush=True);break
  else:
   oracle=source.replace('.ts','.js') if directory=='micros' else source
   node=run([*NODE,oracle,*args],cwd,env,out/(name+'.node'))
   actual=run(['taskset','-c',CPUS,str(out/(name+'.observed')),*args],cwd,env,out/(name+'.check'))
   statuses[name]={'compile_rc':0,'node_rc':node.returncode,'run_rc':actual.returncode,'equal':normalized(node.stdout)==normalized(actual.stdout)}
   print(arm,name,statuses[name],flush=True)
 status_path=out/'status.json'
 previous=json.loads(status_path.read_text()) if status_path.exists() else {}
 previous.update(statuses)
 status_path.write_text(json.dumps(previous,indent=2)+'\n')

def measure(arms, only=None, n=5, diag=False):
 # Independent phase builds must finish before acceptance can start its
 # eight-job stream graph build, or before locked pause measurements.
 pending=ROOT/'tmp/current-phase-builds.pending'
 if pending.exists() and any(arm.startswith('obs-') for arm in arms):
  for arm in ('main','head'):
   log=ROOT/f'keylists2-current72-phase-{arm}-build.log'
   while not log.exists() or 'Finished ' not in log.read_text():
    if log.exists() and 'could not compile' in log.read_text():raise RuntimeError(log)
    time.sleep(10)
 records=[]
 path=EVIDENCE/os.environ.get('KEYLISTS_RECORD_NAME', 'attribution.json' if diag else ('pause-raw.json' if CPUS=='56-63' else 'ab-raw.json'))
 oracle_dir=EVIDENCE/path.stem/'oracle'
 oracle_dir.mkdir(parents=True,exist_ok=True)
 def siblings():
  return {l.split()[0]:[int(x) for x in l.split()[1:]] for l in Path('/proc/stat').read_text().splitlines() if re.match(r'cpu(2[4-9]|3[01]) ',l)}
 for name,(directory,source,args) in PROGRAMS.items():
  if only and name not in only:continue
  if name in ['buffer_heavy','worker_heavy'] and not all(json.loads((EVIDENCE/a/'status.json').read_text()).get(name,{}).get('equal') for a in arms):continue
  cwd=ROOT/'drivers'/directory
  if not all((EVIDENCE/a/(name+'.observed')).exists() for a in arms):continue
  sizes=[200000,50000] if directory=='micros' and not diag else [None]
  for trial in range(n):
   for iterations in sizes:
    argv=[str(iterations)] if iterations is not None else args
    oracle=source.replace('.ts','.js') if directory=='micros' else source
    node=run([*NODE,oracle,*argv],cwd,arm_env(arms[0])[1],oracle_dir/(f'{name}.n{iterations}'))
    assert node.returncode==0,(name,'Node oracle failed',node.stderr[-1000:])
    expected=node.stdout
    for arm in (arms if trial%2==0 else arms[::-1]):
     out=EVIDENCE/arm
     trace_dir=out/path.stem
     trace_dir.mkdir(exist_ok=True)
     dest=trace_dir/(f'{name}.trial{trial}.n{iterations}'+('.diag' if diag else ''))
     _,env=arm_env(arm)
     if diag:env['PERRY_GC_DIAG']='1'
     stat=dest.with_suffix(dest.suffix+'.stat')
     rss=dest.with_suffix(dest.suffix+'.rss')
     exe=out/(name+'.observed')
     before=siblings()
     result=run(['taskset','-c',CPUS,'setarch','-R','perf','stat','-x',';','-e','instructions:u','-o',str(stat),'/usr/bin/time','-f','%M','-o',str(rss),str(exe),*argv],cwd,env,dest)
     after=siblings()
     if result.returncode:raise RuntimeError((arm,name,result.returncode,result.stderr[-1000:]))
     instructions=int(next(l.split(';')[0] for l in stat.read_text().splitlines() if ';instructions:u;' in l))
     obs=re.search(rb'\[keylists-observe\] ([^\n]+)',result.stderr)
     assert obs,(name,result.stderr[-1000:])
     row=dict(arm=arm,name=name,trial=trial,iterations=iterations,instructions=instructions,rss_kb=int(rss.read_text().strip()),**{k:int(v) for k,v in re.findall(rb'(\w+)=(\d+)',obs[1]) for k in [k.decode()]})
     phase=re.search(rb'\[keylists-phases\] ([^\n]+)',result.stderr)
     if phase:row.update({k.decode():int(v) for k,v in re.findall(rb'(\w+)=(\d+)',phase[1])})
     assert row['huge_kb']==0,(arm,name,'THP active')
     row['sibling_busy_pct']={cpu:round(100*(sum(y-x for x,y in zip(before[cpu][:8],after[cpu][:8]))-(after[cpu][3]-before[cpu][3])-(after[cpu][4]-before[cpu][4]))/max(1,sum(y-x for x,y in zip(before[cpu][:8],after[cpu][:8]))),2) for cpu in before}
     assert normalized(result.stdout)==normalized(expected),(arm,name,'oracle mismatch')
     text=subprocess.check_output(['size','-A',str(out/name)]).decode()
     row['text']=int(re.search(r'^\.text\s+(\d+)',text,re.M)[1])
     records.append(row)
     path.write_text(json.dumps(records,indent=2)+'\n')
  print(name,'measured',flush=True)
 path.write_text(json.dumps(records,indent=2)+'\n')

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('mode',choices=['compile','measure','diag']);p.add_argument('arms',nargs='+');p.add_argument('--only',nargs='+');p.add_argument('--n',type=int,default=5);a=p.parse_args()
 if a.mode=='compile':
  for arm in a.arms:compile_arm(arm,a.only)
 else:measure(a.arms,a.only,a.n,a.mode=='diag')
