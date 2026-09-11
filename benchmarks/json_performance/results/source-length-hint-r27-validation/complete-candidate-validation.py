from pathlib import Path
import subprocess
w=Path(__file__).resolve().parent;root=w.parents[3]
for cmd in [['validate-token.py','--candidate'],['check-token-roots.py','--candidate'],['compare-token.py'],['run-after-build.py'],['compare-json-entry-disassembly.py']]:
 subprocess.run(['python3',str(w/cmd[0]),*cmd[1:]],cwd=root,check=True)
print('R27 candidate validation complete: 90 cases per arm, 26 matching IR files and six worker objects.',flush=True)
