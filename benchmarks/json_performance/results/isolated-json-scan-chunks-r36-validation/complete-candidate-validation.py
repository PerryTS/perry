from pathlib import Path
import subprocess
w=Path(__file__).resolve().parent;root=w.parents[3]
for cmd in [['validate-primitive-keys.py','--candidate'],['check-primitive-keys-roots.py','--candidate'],['compare-primitive-keys.py'],['validate-emitters.py','--candidate'],['check-emitter-roots.py','--candidate'],['compare-emitters.py'],['run-after-build.py'],['compare-json-entry-disassembly.py']]:
 subprocess.run(['python3',str(w/cmd[0]),*cmd[1:]],cwd=root,check=True)
print('R36 candidate validation complete: 81 existing cases plus 20 fresh emitter executions plus four primitive-key checks per arm, 28 matching IR files and six worker objects.',flush=True)
