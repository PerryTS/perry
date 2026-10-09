from pathlib import Path
import shutil,json,tarfile,hashlib,base64
L=Path('/root/lanes/perry-coldextract'); S=Path('/root/lanes/perry-upm-score2'); M=L/'micro'; M.mkdir(exist_ok=True)
shutil.copytree('/root/lanes/upm-prof/upm/src',M,dirs_exist_ok=True)
# Corpus remains read-only. Parse before measurement; retain real file windows.
C=Path('/root/lanes/perry-tarloop/micro/corpus'); rows=[]; archives=[]; blob=bytearray()
for tgz in sorted(C.glob('*.tgz')):
 data=tgz.read_bytes(); archives.append(dict(path=str(tgz),size=len(data),integrity='sha512-'+base64.b64encode(hashlib.sha512(data).digest()).decode()))
 with tarfile.open(tgz) as tf:
  files={}
  for f in tf:
   if f.isfile():files[f.name]=f
  for name,f in files.items():
   content=tf.extractfile(f).read(); at=len(blob);blob.extend(content)
   rows.append(dict(path=name,at=at,size=len(content),exec=bool(f.mode&0o111),integrity='sha512-'+base64.b64encode(hashlib.sha512(content).digest()).decode()))
(M/'payload.bin').write_bytes(blob);(M/'files.json').write_text(json.dumps(rows));(M/'archives.json').write_text(json.dumps(archives)); print(len(archives),len(rows),len(blob))
