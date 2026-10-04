import pathlib,json,hashlib,os,shutil,time
B=pathlib.Path('/root/rss-header-20261002'); R=B/'primary-once-advice-main07-v65-ffi-events'; T=R/'target'; E=R/'export'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
assert (B/'gc-once-advice-main07-v65-ffi-events-driver.exit').read_text().strip()=='0'
m=json.loads((B/'once-advice-main07-v65-ffi-events-evidence-files.json').read_text())
assert all(sha(B/n)==h for n,h in m.items())
assert json.loads((E/'independent-verification.json').read_text())['result']['rc']==0
needle=str(T); references=[]
for p in pathlib.Path('/proc').iterdir():
 if not p.name.isdigit():continue
 for n in ['exe','cwd']:
  try:
   value=os.readlink(p/n)
   if value.startswith(needle):references.append([p.name,n,value])
  except OSError:pass
 try:
  if needle in (p/'maps').read_text():references.append([p.name,'maps'])
 except OSError:pass
 try:
  for f in (p/'fd').iterdir():
   try:
    value=os.readlink(f)
    if value.startswith(needle):references.append([p.name,str(f),value])
   except OSError:pass
 except OSError:pass
assert not references,references
before=os.statvfs(B); assert T.is_dir()
shutil.rmtree(T)
after=os.statvfs(B)
proof=dict(removed=str(T),time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),terminal_exit=0,evidence_files_verified=len(m),process_references=references,preserved=['source','export','raw logs','evidence manifest','R31 seed'],free_bytes_before=before.f_bfree*before.f_frsize,free_bytes_after=after.f_bfree*after.f_frsize)
(B/'completed-main07-v65-ffi-cache-cleanup.json').write_text(json.dumps(proof,indent=2)+'\n'); print(json.dumps(proof))
