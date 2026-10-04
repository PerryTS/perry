import pathlib,json,hashlib,os,time,shutil
B=pathlib.Path('/root/rss-header-20261002')
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def free():
 s=os.statvfs(B);return s.f_bfree*s.f_frsize
M=B/'historical-query-auto-v84-manifest.json';m=json.loads(M.read_text());v=json.loads((B/'historical-query-auto-v84-local-verification.json').read_text());A=B/'historical-query-auto-v84.tar.gz'
roots=[B/'primary-lazy-rss-main07-v51/source/target']
assert m['roots']==[str(p) for p in roots]
assert v['all_members_sizes_and_hashes_verified'] and v['verified_files']==len(m['files'])
assert v['manifest_sha256']==sha(M) and len(v['archive_sha256'])==64 and v['archive_bytes']>0
m['archive_sha256']=v['archive_sha256'];m['archive_bytes']=v['archive_bytes']
for job in ('gc-auto-lazy-rss-main07-v52',):
 for suffix in ('.exit','-driver.exit'):assert (B/(job+suffix)).read_text().strip()=='0'
for p in m['proofs']:assert sha(pathlib.Path(p['path']))==p['sha256']
actual={str(p.relative_to(B)) for root in roots for p in root.rglob('*') if p.is_file()}
assert actual==set(m['files'])
for n,e in m['files'].items():
 p=B/n;assert p.stat().st_size==e['bytes'] and sha(p)==e['sha256'],n
references=[]
for proc in pathlib.Path('/proc').iterdir():
 if not proc.name.isdigit():continue
 paths=[proc/'cwd',proc/'exe']
 try:paths.extend((proc/'fd').iterdir())
 except OSError:pass
 for p in paths:
  try:s=os.readlink(p)
  except OSError:continue
  if any(s==str(r) or s.startswith(str(r)+'/') for r in roots):references.append((str(p),s))
 try:
  for line in (proc/'maps').read_text().splitlines():
   if any(str(r)+'/' in line for r in roots):references.append((str(proc/'maps'),line))
 except OSError:pass
assert not references,references
before=free()
for root in roots:shutil.rmtree(root)
# The archive was streamed directly to the local host; no remote copy exists.
r=dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),removed_roots=[str(p) for p in roots],verified_files=len(m['files']),local_archive=v['archive'],archive_sha256=m['archive_sha256'],archive_bytes=m['archive_bytes'],manifest_sha256=sha(M),local_verification_sha256=sha(B/'historical-query-auto-v84-local-verification.json'),free_before_bytes=before,free_after_bytes=free(),active_references=references,scope='Only one terminal historical query experiment auto-build target directory. Sources, exports, app binaries, raw measurement evidence, proofs and shared seeds remain. Exact archive retained locally.',restore='Copy the verified local tar.gz back to /root/rss-header-20261002 and extract it there; every restored file is bound to the retained manifest.')
tmp=B/'historical-query-auto-v84-offload-complete.json.tmp';tmp.write_text(json.dumps(r,indent=2)+'\n');tmp.replace(B/'historical-query-auto-v84-offload-complete.json');print(json.dumps(r))
