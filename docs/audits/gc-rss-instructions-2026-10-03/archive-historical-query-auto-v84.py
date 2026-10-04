import pathlib,json,hashlib,tarfile,time,os
B=pathlib.Path('/root/rss-header-20261002'); jobs=[('gc-auto-lazy-rss-main07-v52',B/'primary-lazy-rss-main07-v51/source/target')]
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
entries={};proofs=[]
for v,root in jobs:
 assert (B/(v+'.exit')).read_text().strip()=='0' and (B/(v+'-driver.exit')).read_text().strip()=='0'
 proof=B/(v+'-verification.json');assert json.loads(proof.read_text())['all_raw_sources_products_oracles_counters_verified'];proofs.append(dict(path=str(proof),sha256=sha(proof)))
 products=json.loads((B/(v+'-auto-runtime-products.json')).read_text())
 for stamp,row in products.items():
  if not pathlib.Path(stamp).is_relative_to(root):continue
  assert pathlib.Path(stamp).read_text()==row['stamp']
  assert all(sha(pathlib.Path(n))==h for n,h in row['archives'].items())
 for p in root.rglob('*'):
  if p.is_file():entries[str(p.relative_to(B))]=dict(sha256=sha(p),bytes=p.stat().st_size)
assert entries
manifest=dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),roots=[str(r) for _,r in jobs],proofs=proofs,files=entries,scope='Completed historical query experiment auto products only; archive streamed locally before any removal.')
(B/'historical-query-auto-v84-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
import sys
with tarfile.open(fileobj=sys.stdout.buffer,mode='w|gz',compresslevel=1) as t:
 for n in sorted(entries):t.add(B/n,arcname=n)
