import pathlib,json,hashlib,tarfile,time,os
B=pathlib.Path('/root/rss-header-20261002'); jobs=[('gc-auto-learned-floor-main07-v60',B/'primary-learned-floor-main07-v59/source/target'),('gc-auto-map-prefix-main07-v69r2',B/'primary-map-prefix-main07-v68/source/target')]
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
archive=B/'rejected-auto-products-v78.tar.gz'
assert not archive.exists()
with tarfile.open(archive,'w:gz',compresslevel=1) as t:
 for n in sorted(entries):t.add(B/n,arcname=n)
manifest=dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),roots=[str(r) for _,r in jobs],proofs=proofs,files=entries,archive_sha256=sha(archive),archive_bytes=archive.stat().st_size,scope='Archive only completed rejected experiments; original artifacts remain until durable local copy and stream hash verification. No source, exports, app binaries, raw measurements, or shared seeds are removed.')
(B/'rejected-auto-products-v78-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print(json.dumps({k:v for k,v in manifest.items() if k!='files'}))
