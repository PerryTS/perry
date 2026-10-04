import pathlib,json,hashlib,tarfile,time
B=pathlib.Path(__file__).resolve().parent
M=B/'historical-query-auto-v84-manifest.json'; A=B/'historical-query-auto-v84.tar.gz'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
m=json.loads(M.read_text()); m['archive_bytes']=A.stat().st_size; m['archive_sha256']=sha(A)
seen={}; links={}
with tarfile.open(A,'r|gz') as t:
 for e in t:
  assert e.name in m['files'] and e.name not in seen and e.name not in links,e.name
  if e.islnk():
   assert e.linkname in m['files'],e.linkname
   links[e.name]=e.linkname
  else:
   assert e.isfile(),(e.name,e.type)
   with t.extractfile(e) as f: h=hashlib.file_digest(f,'sha256').hexdigest()
   seen[e.name]={'bytes':e.size,'sha256':h}; assert seen[e.name]==m['files'][e.name],e.name
for n,target in links.items():
 assert target in seen and seen[target]==m['files'][n],(n,target)
 seen[n]=seen[target]
assert set(seen)==set(m['files'])
r=dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),archive=str(A),archive_bytes=m['archive_bytes'],archive_sha256=m['archive_sha256'],manifest_sha256=sha(M),verified_files=len(seen),verified_hardlinks=len(links),all_members_sizes_and_hashes_verified=True,method='Streaming verification without extraction; TAR hardlinks bind to separately hashed regular members.')
(B/'historical-query-auto-v84-local-verification.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r))
