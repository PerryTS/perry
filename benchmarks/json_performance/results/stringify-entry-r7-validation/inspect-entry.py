from pathlib import Path
import hashlib,json,subprocess
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent;r5=w.parent/'invariant-field-loop-r5'
records=[]
for arm,base in [('r5',r5),('candidate',w)]:
 binary=base/'worker'
 symbols=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm','--defined-only',str(binary)],text=True)
 selected=[line.split()[-1] for line in symbols.splitlines() if 'stringify_full_fallback' in line or line.split()[-1]=='_js_json_stringify_full']
 assert '_js_json_stringify_full' in selected
 if arm=='candidate':assert len([s for s in selected if s.endswith('23stringify_full_fallback')])==1,selected
 cmd=['/opt/homebrew/opt/llvm/bin/llvm-objdump','--disassemble','--no-show-raw-insn','--disassemble-symbols='+','.join(selected),str(binary)]
 assembly=subprocess.check_output(cmd,text=True);(w/(arm+'-stringify-full-machine.s')).write_text(assembly)
 records.append({'arm':arm,'symbols':selected,'command':cmd,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()})
objects=[]
for name in ['worker.o','access-worker.o','rotating-worker.o']:
 a=hashlib.sha256((r5/name).read_bytes()).hexdigest();b=hashlib.sha256((w/name).read_bytes()).hexdigest();objects.append({'name':name,'r5_sha256':a,'candidate_sha256':b,'same':a==b})
assert all(r['same'] for r in objects),objects
(w/'entry-machine.json').write_text(json.dumps({'binaries':records,'objects':objects},indent=2)+'\n')
print('PASS all three generated worker objects identical to R5; linked entry/helper assembly retained')
