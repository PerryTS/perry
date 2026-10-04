import pathlib,json,hashlib,os,time
B=pathlib.Path('/root/rss-header-20261002')
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
selected={};build_manifests=[]
for manifest in sorted(B.glob('*-builds.json')):
 try:rows=json.loads(manifest.read_text())
 except (ValueError,OSError):continue
 if not isinstance(rows,list):continue
 used=False
 for row in rows:
  if not isinstance(row,dict) or not all(k in row for k in ['cmd','symbol_sha256','binary_sha256','text_sha256']):continue
  if row.get('rc')!=0 or row.get('reason') is not None:continue
  symbol=pathlib.Path(row['cmd'][-1])
  if not symbol.name.endswith('-symbols'):continue
  plain=pathlib.Path(str(symbol).removesuffix('-symbols'))
  dumps=[B/(file.name+'.text') for file in [symbol,plain]]
  if not any(p.exists() for p in dumps):continue
  assert symbol.is_relative_to(B) and plain.is_relative_to(B)
  assert sha(symbol)==row['symbol_sha256'] and sha(plain)==row['binary_sha256']
  for file,dump in zip([symbol,plain],dumps):
   if dump.exists():
    assert sha(dump)==row['text_sha256'],dump
    selected[str(dump)]=dict(sha256=row['text_sha256'],bytes=dump.stat().st_size,source_binary=str(file))
    used=True
 if used:build_manifests.append(dict(path=str(manifest),sha256=sha(manifest)))
assert selected
before=os.statvfs(B)
for n in selected:pathlib.Path(n).unlink()
after=os.statvfs(B)
proof=dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),build_manifests=build_manifests,removed=selected,removed_bytes=sum(x['bytes'] for x in selected.values()),preserved=['source','binaries','symbol binaries','archives','stamps','raw evidence','proofs'],free_before=before.f_bfree*before.f_frsize,free_after=after.f_bfree*after.f_frsize)
(B/'verified-auto-text-dumps-v73-cleanup.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps({k:v for k,v in proof.items() if k!='removed'}))
