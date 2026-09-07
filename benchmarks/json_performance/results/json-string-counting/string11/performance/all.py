from pathlib import Path
import subprocess,sys
p=Path(__file__).resolve().parent
for name in ['recheck.py','lifetimes_recheck.py']:
 subprocess.run([sys.executable,str(p/name)],check=True,cwd=p)
