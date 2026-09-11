from pathlib import Path
import hashlib,json,shutil,subprocess,time
w=Path(__file__).resolve().parent;root=w.parents[3]
original_root=Path('/Users/amlug/projects/perry/json-stringify-key-snapshot-r29')
original=original_root/'benchmarks/json_performance/.work/integer-remainder-r32'
meta=json.loads((original/'build-provenance.json').read_text())
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
assert meta['source_commit']==head==subprocess.check_output(['git','rev-parse','HEAD'],cwd=original_root,text=True).strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
assert not subprocess.check_output(['git','status','--porcelain'],cwd=original_root)
assert meta['command']==['cargo','build','--release','-p','perry','-p','perry-runtime-static','-p','perry-stdlib-static']
start=json.loads((original/'build-start.json').read_text());assert start['source_commit']==head
assert json.loads((original/'build-command-result.json').read_text())['exit_code']==0
units=json.loads((original/'unit-source.json').read_text());assert units['source_commit']==head and units['exit_code']==0
assert units['env']=={'RUST_TEST_THREADS':'1'}
arithmetic=json.loads((original/'arithmetic-suite-provenance.json').read_text());assert arithmetic['source_commit']==head and arithmetic['exit_code']==0
for p,h in units['hashes'].items():
 assert hashlib.sha256((root/p).read_bytes()).hexdigest()==hashlib.sha256((original_root/p).read_bytes()).hexdigest()==h
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();records=[]
def copy(p,d):
 assert not d.exists(),d
 d.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,d);assert sha(p)==sha(d)
 records.append(dict(source=str(p),copy=str(d.relative_to(root)),sha256=sha(d)))
for name,details in meta['files'].items():
 p=original/'frozen-build'/name;assert sha(p)==details['sha256'] and p.stat().st_size==details['bytes'] and details['mtime']>start['started_unix']
 copy(p,w/'frozen-build'/name)
for n in ['build-provenance.json','build-start.json','build-command-result.json','unit-source.json','unit.log','build.log','validate-and-build.log','lint-results.json','script-lint.log','file-cap.log','worktree-provenance.json','lint-review.json','arithmetic-suite.log','arithmetic-suite-provenance.json','build-resume.json','build-controller.log','hold-production-reviewed.txt']:
 copy(original/n,w/n)
for n in ['build-release.py','validate-and-build.py','run-lint.py']:
 # Retain the bootstrap controller if its bytes differ from the actual builder.
 p=w/n
 if p.exists():
  saved=w/'builder-controller-before-copy'/n;saved.parent.mkdir(exist_ok=True);assert not saved.exists();p.rename(saved)
 copy(original/n,p)
for p in (original/'initial-db78-validation').rglob('*'):
 if p.is_file():copy(p,w/'initial-db78-validation'/p.relative_to(original/'initial-db78-validation'))
(w/'build-copy-provenance.json').write_text(json.dumps(dict(source_commit=head,actual_build_worktree=str(original_root),validation_worktree=str(root),copied_unix=time.time(),note='Normal all-three-package production build from a clean committed separate worktree; both source trees match. Candidate timing/validation uses these immutable artifacts. Shared mutable target directory is not used for worker linking.',files=records),indent=2)+'\n')
print('Verified/copy-froze',len(records),'build, unit and lint payloads from',head)
