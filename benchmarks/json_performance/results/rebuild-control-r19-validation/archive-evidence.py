from pathlib import Path
import gzip,hashlib,json
w=Path(__file__).resolve().parent
out=w.parents[1]/'results/rebuild-control-r19-validation'
out.mkdir(exist_ok=False)
rows=[]
for source in sorted(w.iterdir()):
 if not source.is_file() or source.suffix not in {'.json','.py','.log','.s','.diff','.md','.txt'}:
  continue
 raw=source.read_bytes(); compressed=source.suffix in {'.log','.s','.diff'}
 name=source.name+('.gz' if compressed else '')
 data=gzip.compress(raw,compresslevel=9,mtime=0) if compressed else raw
 (out/name).write_bytes(data)
 assert (gzip.decompress(data) if compressed else data)==raw
 rows.append(dict(path=name,original_path=source.name,original_bytes=len(raw),original_sha256=hashlib.sha256(raw).hexdigest(),sha256=hashlib.sha256(data).hexdigest(),compression='gzip' if compressed else None))
manifest=dict(source_commit=json.loads((w/'build-provenance.json').read_text())['source_commit'],note='Unchanged-main independent rebuild; three byte-identical artifacts. No new timed or diagnostic remote window; no runtime candidate.',files=rows)
(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
for row in rows:
 data=(out/row['path']).read_bytes();assert hashlib.sha256(data).hexdigest()==row['sha256']
 raw=gzip.decompress(data) if row['compression'] else data
 assert len(raw)==row['original_bytes'] and hashlib.sha256(raw).hexdigest()==row['original_sha256']
print('Archived and verified',len(rows),'artifacts;',sum((out/r['path']).stat().st_size for r in rows),'bytes.')
