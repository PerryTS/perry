from pathlib import Path
import subprocess
w=Path(__file__).resolve().parent;root=w.parents[3]
for name in ['compare-primitive-keys.py','validate-emitters.py','check-emitter-roots.py','compare-emitters.py','run-after-build.py','compare-json-entry-disassembly.py']:
 args=['--candidate'] if name in ['validate-emitters.py','check-emitter-roots.py'] else []
 subprocess.run(['python3',str(w/name),*args],cwd=root,check=True)
print('R38 candidate validation complete; primitive native finding retained explicitly in both arms.',flush=True)
