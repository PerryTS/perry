from pathlib import Path
import datetime,hashlib,json
w=Path(__file__).resolve().parent
original=json.loads((w/'main-build-provenance.json').read_text())
rebuilt=json.loads((w/'build-provenance.json').read_text())
assert original['source_commit']==rebuilt['source_commit']
assert original['command']==rebuilt['command']
for directory,record in [('frozen-main',original),('frozen-build',rebuilt)]:
 start=datetime.datetime.fromisoformat(record['started_utc']).timestamp()
 for name,meta in record['files'].items():
  data=(w/directory/name).read_bytes()
  assert meta['mtime']>start
  assert len(data)==meta['bytes']
  assert hashlib.sha256(data).hexdigest()==meta['sha256']
  assert meta['sha256']==original['files'][name]['sha256']
print('Both builds: same source, command, fresh outputs, identical artifact hashes.')
