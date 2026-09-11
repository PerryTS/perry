from pathlib import Path
import gzip,hashlib,json
w=Path(__file__).resolve().parent;d=w.parents[1]/('results/'+w.name+'-validation')
m=json.loads((d/'manifest.json').read_text());previous={e['original_path']:e for e in m['files']};entries=[];changed=[]
allowed={'.py','.json','.ts','.log','.stdout','.stderr','.ll','.md','.rs','.txt','.diff','.patch','.js','.s','.gz','.commit'}
for p in sorted(w.rglob('*')):
 if p.name in {'next-read.patch','cached-read-next.rs','next-read-candidate.md'}:continue
 if not p.is_file() or (p.suffix not in allowed and p.name != 'hold-production') or '__pycache__' in p.parts:continue
 rel=str(p.relative_to(w));raw=p.read_bytes();digest=hashlib.sha256(raw).hexdigest();old=previous.get(rel)
 if old and old['original_sha256']==digest:entries.append(old);continue
 compressed=p.suffix in {'.log','.stdout','.stderr','.ll','.s','.patch','.diff'} or p.name.endswith('.sample.txt') or p.name in {'callback-only.ts','run-zero-retained.py'}
 name=rel+('.gz' if compressed else '');data=gzip.compress(raw,compresslevel=9,mtime=0) if compressed else raw;target=d/name;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(data)
 entries.append(dict(path=name,original_path=rel,original_bytes=len(raw),original_sha256=digest,sha256=hashlib.sha256(data).hexdigest(),compression='gzip' if compressed else None));changed.append(rel)
assert set(previous)<=set(e['original_path'] for e in entries)
m['files']=entries;(d/'manifest.json').write_text(json.dumps(m,indent=2)+'\n')
for e in entries:
 data=(d/e['path']).read_bytes();assert hashlib.sha256(data).hexdigest()==e['sha256'];raw=gzip.decompress(data) if e['compression'] else data;assert len(raw)==e['original_bytes'] and hashlib.sha256(raw).hexdigest()==e['original_sha256']
print('VERIFIED',len(entries),'validation artifacts; refreshed',changed)
