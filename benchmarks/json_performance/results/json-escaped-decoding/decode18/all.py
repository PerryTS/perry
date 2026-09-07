from pathlib import Path
import subprocess,sys
p=Path(__file__).resolve().parent
for script in ['focused.py','lifetimes_recheck.py']:
 subprocess.run([sys.executable,str(p/script)],check=True)
