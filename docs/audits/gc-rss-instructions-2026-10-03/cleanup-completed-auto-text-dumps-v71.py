import pathlib,json,hashlib,os,time
B=pathlib.Path('/root/rss-header-20261002')
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
selected={};prefixes=['gc-auto-main07-v40','gc-auto-entry-once-main07-v46','gc-auto-lazy-rss-main07-v52','gc-auto-learned-floor-main07-v60']
for v in prefixes:
 assert (B/(v+'.exit')).read_text().strip()=='0'
 assert (B/(v+'-verification.json')).is_file(),v
 for row in json.loads((B/(v+'-builds.json')).read_text()):
  symbol=pathlib.Path(row['cmd'][-1]);assert symbol.name.endswith('-symbols'),row
  plain=pathlib.Path(str(symbol).removesuffix('-symbols'))
  assert row['rc']==0 and row['reason'] is None and sha(symbol)==row['symbol_sha256'] and sha(plain)==row['binary_sha256']
  for file in [symbol,plain]:
   dump=B/(file.name+'.text')
   if dump.exists():
    assert sha(dump)==row['text_sha256'],dump
    selected[str(dump)]=dict(sha256=row['text_sha256'],bytes=dump.stat().st_size,source_binary=str(file))
assert selected
before=os.statvfs(B)
for n in selected:pathlib.Path(n).unlink()
after=os.statvfs(B)
proof=dict(time_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),removed=selected,removed_bytes=sum(x['bytes'] for x in selected.values()),preserved=['source','binaries','symbol binaries','archives','stamps','raw evidence','proofs'],free_before=before.f_bfree*before.f_frsize,free_after=after.f_bfree*after.f_frsize)
(B/'completed-auto-text-dumps-v71-cleanup.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps({k:v for k,v in proof.items() if k!='removed'}))
