from pathlib import Path
import hashlib,json,shutil,subprocess
w=Path(__file__).resolve().parent;root=w.parents[3]
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
pre=json.loads((w/'build-provenance.json').read_text());assert pre['source_commit']==subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
for n in ['frozen-build','build-provenance.json','build.log','unit-source.json','unit.log','script-lint-source.json','script-lint.log','file-cap.log']:
 (w/n).rename(w/('pre-census-'+n))
path='scripts/shape_descriptor_census_baseline.json';shutil.copy2(w/'reviewed-shape-census-baseline.json',root/path)
cmd=['python3','scripts/shape_descriptor_census.py']
with (w/'shape-census-final.log').open('wb') as log:subprocess.run(cmd,cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True)
subprocess.run(['git','add','--',path],cwd=root,check=True);subprocess.run(['git','diff','--cached','--check'],cwd=root,check=True);subprocess.run(['git','commit','--quiet','-m','chore(json): refresh exact inventory for template capture fields'],cwd=root,check=True)
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert subprocess.check_output(['git','diff','--name-only',pre['source_commit'],head],cwd=root,text=True).splitlines()==[path]
# Force all three production outputs to be emitted after the new build start.
# Only mtimes change; no source bytes or compiler options change.
refreshed=['crates/perry-runtime-static/src/lib.rs','crates/perry-stdlib-static/src/lib.rs','crates/perry/src/main.rs']
hashes={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in refreshed}
for p in refreshed:(root/p).touch()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
record={'preliminary_source':pre['source_commit'],'source_commit':head,'changed_paths':[path],'inventory_gate_command':cmd,'inventory_gate_exit_code':0,'freshness_refresh_mtime_only':hashes,'note':'No runtime source changed; preliminary artifacts are preserved and never timed. Units and the exact production build rerun from the final clean source.'}
(w/'census-finalization.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record,indent=2),flush=True)
subprocess.run(['python3',str(w/'validate-and-build.py')],cwd=root,check=True)
