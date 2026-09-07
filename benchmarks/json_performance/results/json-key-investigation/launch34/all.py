from pathlib import Path
import subprocess,sys,os
p=Path(__file__).resolve().parent
os.chdir(Path.home()/'json-escape-runtime-v18-20260907-codex')
for script in ['recheck.py','lifetimes_recheck.py']:
 subprocess.run([sys.executable,str(p/script)],check=True)
