from pathlib import Path
import datetime, hashlib, json, os, shutil, subprocess, time
root=Path(__file__).resolve().parents[4];work=Path(__file__).resolve().parent
commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
command=['cargo','build','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static']
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
previous=work.parent/'invariant-field-loop-r5/frozen-build'
archive_hashes={name:hashlib.sha256((previous/name).read_bytes()).hexdigest() for name in ['libperry_runtime.a','libperry_stdlib.a']}
assert not subprocess.check_output(['git','diff','24a8ab695',commit,'--','crates/perry-runtime','crates/perry-runtime-static','crates/perry-stdlib','crates/perry-stdlib-static','Cargo.toml','Cargo.lock'],cwd=root), 'R6 must not change runtime/archive source'
started=time.time()
with (work/'build.log').open('wb') as log:
 result=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
if result.returncode:raise SystemExit(result.returncode)
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==commit
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
frozen=work/'frozen-build';frozen.mkdir(exist_ok=False)
files={}
for name in ['perry','libperry_runtime.a','libperry_stdlib.a']:
 source=root/'target/release'/name; dest=frozen/name
 if name == 'perry': assert source.stat().st_mtime>started,(name,'stale compiler')
 else: assert hashlib.sha256(source.read_bytes()).hexdigest()==archive_hashes[name], (name,'runtime archive changed unexpectedly')
 shutil.copy2(source,dest)
 sha=hashlib.sha256(source.read_bytes()).hexdigest()
 assert hashlib.sha256(dest.read_bytes()).hexdigest()==sha
 files[name]={'sha256':sha,'bytes':source.stat().st_size,'mtime':source.stat().st_mtime}
meta={'archive_reference_commit':'24a8ab6953ba811f5112e49ba1c79ae6e683118b','archive_policy':'No runtime source change; both archives must equal R5 frozen three-package build exactly','source_commit':commit,'command':command,'started_utc':datetime.datetime.fromtimestamp(started,datetime.timezone.utc).isoformat(),'elapsed_seconds':time.time()-started,'files':files}
(work/'build-provenance.json').write_text(json.dumps(meta,indent=2)+'\n')
print(json.dumps(meta,indent=2),flush=True)
