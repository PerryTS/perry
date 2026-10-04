import pathlib,json,hashlib,os,time,shutil
B=pathlib.Path('/root/rss-header-20261002')
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def free():
 s=os.statvfs(B);return s.f_bfree*s.f_frsize
M=B/'rejected-auto-products-v78-manifest.json';m=json.loads(M.read_text());v=json.loads((B/'rejected-auto-products-v78-local-verification.json').read_text());A=B/'rejected-auto-products-v78.tar.gz'
roots=[B/'primary-learned-floor-main07-v59/source/target',B/'primary-map-prefix-main07-v68/source/target']
assert m['roots']==[str(p) for p in roots]
assert v['all_members_sizes_and_hashes_verified'] and v['verified_files']==len(m['files'])
assert v['archive_sha256']==m['archive_sha256']==sha(A) and v['manifest_sha256']==sha(M)
for job in ('gc-auto-learned-floor-main07-v60','gc-auto-map-prefix-main07-v69r2'):
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
A.unlink()
r=dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),removed_roots=[str(p) for p in roots],verified_files=len(m['files']),local_archive=v['archive'],archive_sha256=m['archive_sha256'],archive_bytes=m['archive_bytes'],manifest_sha256=sha(M),local_verification_sha256=sha(B/'rejected-auto-products-v78-local-verification.json'),free_before_bytes=before,free_after_bytes=free(),active_references=references,scope='Only two terminal, rejected experiment auto-build target directories and the verified remote archive copy. Sources, exports, app binaries, raw measurement evidence, proofs and shared seeds remain. Exact archive retained locally.',restore='Copy the verified local tar.gz back to /root/rss-header-20261002 and extract it there; every restored file is bound to the retained manifest.')
tmp=B/'rejected-auto-products-v78-offload-complete.json.tmp';tmp.write_text(json.dumps(r,indent=2)+'\n');tmp.replace(B/'rejected-auto-products-v78-offload-complete.json');print(json.dumps(r))
