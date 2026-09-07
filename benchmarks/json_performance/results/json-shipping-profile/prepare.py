from pathlib import Path
import hashlib,json,shutil,subprocess,sys,tomllib,os
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');repo=art.parent/'codex-json-gc-deferral';target=art.parent/'codex-json-changed-target/release';arm=sys.argv[1]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
stamp=json.loads((art/(arm+'-source-stamp.json')).read_text());assert all(sha(repo/f)==s for f,s in stamp.items())
old=json.loads((art/'record-bytes-reference-build-settings.json').read_text());settings={}
for name,expected in old.items():
 f=max((target/'build'/name).glob('*/fingerprint/lib-*.json'),key=lambda p:p.stat().st_mtime);v=json.loads(f.read_text());settings[name]={k:v[k] for k in expected}
 assert all(v[k]==expected[k] for k in expected if k!='profile'),(name,v,expected)
assert settings['perry-runtime']['profile']==16677950474920519499
assert settings['perry-stdlib']['profile']==9094639109684260183
for k in ['CARGO_PROFILE_RELEASE_CODEGEN_UNITS','CARGO_PROFILE_RELEASE_LTO','CARGO_PROFILE_RELEASE_OPT_LEVEL','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS']:assert os.environ.get(k) is None,k
profiles=tomllib.loads((repo/'Cargo.toml').read_text())['profile'];release=profiles['release'];dist=profiles['dist'];effective={}
for name in old:
 r={k:v for k,v in release.items() if k!='package'};r.update(release.get('package',{}).get(name,{}))
 d={k:v for k,v in release.items() if k!='package'};d.update({k:v for k,v in dist.items() if k not in ['inherits','package']});d.update(dist.get('package',{}).get(name,{}))
 assert r==d,(name,r,d);effective[name]=r
 assert r['codegen-units']==(16 if name.endswith('-static') else 1)
dest=art/(arm+'-runtime');dest.mkdir()
for name in ['libperry_runtime.a','libperry_stdlib.a']:shutil.copy2(target/name,dest/name)
workers={}
for obj,suffix in [('worker.o','worker'),('lifetime-worker.o','lifetime-worker')]:
 worker=art/(arm+'-'+suffix);cmd=['cc',str(art/obj),str(dest/'libperry_runtime.a'),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(worker)];subprocess.run(cmd,check=True)
 workers[suffix]=dict(path=str(worker),sha256=sha(worker),object_sha256=sha(art/obj),link_command=cmd)
record=dict(arm=arm,source_sha256=stamp,settings=settings,effective_manifest_profiles=effective,release_matches_dist=True,workspace_manifest_sha256=sha(repo/'Cargo.toml'),cargo_config_sha256=sha(repo/'.cargo/config.toml'),archives={n:sha(dest/n) for n in ['libperry_runtime.a','libperry_stdlib.a']},workers=workers,build_command=['cargo','build','--release','-p','perry-runtime-static','-p','perry-stdlib-static','--target-dir',str(target.parent)])
(art/(arm+'-provenance.json')).write_text(json.dumps(record,indent=2)+'\n');print('PINNED',arm,{k:v['sha256'] for k,v in workers.items()},flush=True)
