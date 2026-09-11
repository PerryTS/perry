from pathlib import Path
import datetime, hashlib, json, os, shutil, subprocess, time, sys
main = "--main" in sys.argv
root=Path(__file__).resolve().parents[4];work=Path(__file__).resolve().parent
commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
if (work/'hold-production').exists():raise SystemExit((work/'hold-production').read_text())
command=['cargo','build','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static']
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
# Refresh only mtimes so cargo emits all three production artifacts in this window.
refreshed=['crates/perry/src/main.rs','crates/perry-runtime-static/src/lib.rs','crates/perry-stdlib-static/src/lib.rs']
for p in refreshed:os.utime(root/p,None)
started=time.time()
(work/'build-start.json').write_text(json.dumps({'source_commit':commit,'started_unix':started,'command':command,'mtime_only_refresh':refreshed},indent=2)+'\n')
with (work/('main-build.log' if main else 'build.log')).open('wb') as log:
 result=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
(work/'build-command-result.json').write_text(json.dumps({'exit_code':result.returncode,'elapsed_seconds':time.time()-started},indent=2)+'\n')
if result.returncode:raise SystemExit(result.returncode)
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==commit
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
frozen=work/('frozen-main' if main else 'frozen-build');frozen.mkdir(exist_ok=False)
files={}
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 source=root/'target/release'/name; dest=frozen/name
 assert source.stat().st_mtime>started,(name,'stale production artifact')
 shutil.copy2(source,dest)
 sha=hashlib.sha256(source.read_bytes()).hexdigest()
 assert hashlib.sha256(dest.read_bytes()).hexdigest()==sha
 files[name]={'sha256':sha,'bytes':source.stat().st_size,'mtime':source.stat().st_mtime}
meta={'source_commit':commit,'command':command,'started_utc':datetime.datetime.fromtimestamp(started,datetime.timezone.utc).isoformat(),'elapsed_seconds':time.time()-started,'files':files,'mtime_only_refresh':refreshed}
(work/('main-build-provenance.json' if main else 'build-provenance.json')).write_text(json.dumps(meta,indent=2)+'\n')
print(json.dumps(meta,indent=2),flush=True)
