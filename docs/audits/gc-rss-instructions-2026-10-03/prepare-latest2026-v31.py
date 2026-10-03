"""Immutable freshly fetched main and independently hashed GC overlay inputs."""
import pathlib,json,hashlib,tarfile,subprocess
B=pathlib.Path('/Users/amlug/projects/perry/secret-tests/scratchpad/rss-header-20261002')
W=pathlib.Path('/Users/amlug/projects/perry/rss-gc-runtime-latest2026-20261003')
BASE='2026ecfe6dd9df1a0a8616e3bc5c5561e8e0cf63'
HEAD=subprocess.check_output(['git','-C',W,'rev-parse','HEAD'],text=True).strip()
assert HEAD=='6c761801fa4c76ad79b67bbc4338bee7739093b7'
assert not subprocess.check_output(['git','-C',W,'status','--porcelain'],text=True)
def sha(p):
    with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
archive=B/'perry-latest2026-v31-source.tar.gz';base_inputs={}
with tarfile.open(archive) as t:
    for f in t:
        if f.isfile() and (f.name.endswith('.rs') or pathlib.Path(f.name).name in ['Cargo.toml','Cargo.lock']):
            base_inputs[f.name]=hashlib.sha256(t.extractfile(f).read()).hexdigest()
paths=subprocess.check_output(['git','-C',W,'diff','--name-only',BASE,HEAD,'--','crates'],text=True).splitlines()
assert len(paths)==22 and all(n.startswith('crates/perry-runtime/src/') for n in paths)
overlay={n:sha(W/n) for n in paths};gc_inputs=base_inputs|overlay
with tarfile.open(B/'gc-runtime-latest2026-v31-overlay.tar.gz','w:gz') as t:
    for n in paths:t.add(W/n,arcname=n)
assert all(sha(W/n)==h for n,h in gc_inputs.items())
for arm,data in [('base',base_inputs),('gc',gc_inputs)]:
    (B/('latest2026-v31-'+arm+'-inputs.json')).write_text(json.dumps(data,indent=2)+'\n')
proof=dict(base=BASE,head=HEAD,authored_runtime_paths=22,files=overlay,baseline_archive_sha256=sha(archive),overlay_archive_sha256=sha(B/'gc-runtime-latest2026-v31-overlay.tar.gz'),manifest_hashes={a:sha(B/('latest2026-v31-'+a+'-inputs.json')) for a in ['base','gc']},scope='Newly fetched main and separate GC rebase; c170 measurements are not relabeled. Private idle-Eden probe excluded.')
(B/'provenance-latest2026-v31.json').write_text(json.dumps(proof,indent=2)+'\n')
print('Immutable latest main inputs',len(base_inputs),'proposal inputs',len(gc_inputs),'overlay paths',len(overlay))
