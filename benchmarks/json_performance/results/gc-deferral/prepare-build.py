from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path('/Users/amlug/projects/perry/codex-json-gc-deferral')
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts')
target=Path('/Users/amlug/projects/perry/codex-json-changed-target/release')
def digest(p): return hashlib.sha256(p.read_bytes()).hexdigest()
stamp=json.loads((art/'defer2-source-stamp.json').read_text())
assert {p:digest(root/p) for p in stamp}==stamp
settings=json.loads((art/'record-bytes-reference-build-settings.json').read_text());actual={}
for name,expected in settings.items():
 f=next((target/'build'/name).glob('*/fingerprint/lib-*.json'));v=json.loads(f.read_text());actual[name]={k:v[k] for k in expected}
assert actual==settings,(actual,settings)
dest=art/'defer2-runtime';dest.mkdir()
for name in ['libperry_runtime.a','libperry_stdlib.a']:shutil.copy2(target/name,dest/name)
worker=art/'defer2-worker';link=['cc',str(art/'worker.o'),str(dest/'libperry_runtime.a'),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(worker)]
subprocess.run(link,check=True)
record={'parent_commit':'fb69a70596d24a80249c77952521dc4da63b3582','source_worktree':str(root),'source_sha256':stamp,'verified_build_settings':actual,'worker_sha256':digest(worker),'application_object_sha256':digest(art/'worker.o'),'runtime_sha256':digest(dest/'libperry_runtime.a'),'stdlib_sha256':digest(dest/'libperry_stdlib.a'),'reference_worker_sha256':digest(art/'batch-worker'),'link_command':link,'note':'Bounded JSON nursery-GC deferral experiment on the batch-builder parent. Explicitly authorized scheduling changes; completion records debt without collecting, nursery grace expires on bytes or polls, emergency and old pressure bypass. No tracing or object layout changes.'}
assert record['application_object_sha256']=='28a4483ebffc124debdb5e223f4972cf9cdb27bdae59476981539cd7a27b5618'
(art/'defer2-provenance.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({k:v for k,v in record.items() if k not in ['source_sha256','verified_build_settings']},indent=2))
