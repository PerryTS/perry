from pathlib import Path
import subprocess
w=Path(__file__).resolve().parent
for step in [['build-workers.py'],['inspect-machine.py'],['inspect-cached.py'],['compare-cached-machine.py'],['inspect-capture.py'],['validate-fixtures.py'],['validate-options.py','--candidate'],['probe-lazy-baseline.py','--candidate'],['validate-fraction.py']]:
 subprocess.run(['python3',str(w/step[0])]+step[1:],check=True)
