import pathlib,json,hashlib,tarfile
B=pathlib.Path('/root/rss-header-20261002')
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
files=set()
for v in ['gc-auto-map-prefix-main07-v69','gc-map-prefix-main07-v70-linked']:
 assert (B/(v+'-driver.exit')).read_text().strip()=='1'
 files.add(B/(v+'-driver.exit'))
 files.update(p for p in B.glob(v+'-*') if p.is_file())
 files.update(B/'logs'/p.name for p in (B/'logs').glob(v+'-*') if p.is_file())
files.update([B/'baseline-map-crosscheck-v70-triage.json',B/'triage-baseline-map-crosscheck-v70.py',B/'completed-auto-text-dumps-v71-cleanup.json',B/'cleanup-completed-auto-text-dumps-v71.py',pathlib.Path(__file__)])
for n in ['primary-map-prefix-main07-v69-auto.py','primary-map-prefix-main07-v69-auto-run.sh','primary-map-prefix-main07-v70-linked.py','primary-map-prefix-main07-v70-linked-run.sh']:files.add(B/n)
m=B/'map-prefix-main07-v69-v70-failure-r1-evidence-files.json';m.write_text(json.dumps({str(p.relative_to(B)):sha(p) for p in sorted(files)},indent=2)+'\n')
with tarfile.open(B/'map-prefix-main07-v69-v70-failure-r1-evidence.tar.gz','w:gz') as t:
 for p in sorted(files|{m}):t.add(p,arcname=str(p.relative_to(B)))
print('Preserved',len(files),'original failure and diagnostic evidence files')
