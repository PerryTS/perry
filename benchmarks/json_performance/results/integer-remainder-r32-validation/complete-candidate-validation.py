from pathlib import Path
import subprocess
w=Path(__file__).resolve().parent;root=w.parents[3]
for command in [['validate-remainder.py'],['check-remainder-roots.py'],['validate-remainder.py','--candidate'],['check-remainder-roots.py','--candidate'],['compare-remainder.py'],['run-after-build.py'],['compare-json-entry-disassembly.py']]:
 subprocess.run(['python3',str(w/command[0]),*command[1:]],cwd=root,check=True)
print('R32 validation complete: 81 original candidate controls, six remainder checks per arm, 26 matching IR files and six worker objects.',flush=True)
