from pathlib import Path
import hashlib,json,re,subprocess
root=Path(__file__).resolve().parents[4];w=Path(__file__).resolve().parent;r5=w.parent/'invariant-field-loop-r5'
records=[]
for arm,base in [('r5',r5),('r6',w)]:
 source=base/'ir-native/access-worker/.perry-trace/llvm/access_worker_ts.ll'
 ir=source.read_text();slots=set(re.findall(r'call double @js_packed_arraylike_index_get\([^\n]*, ptr (@[^)]+)\)',ir))
 assert slots, 'No dispatched indexing found'
 block='';loads=[]
 for line in ir.splitlines():
  if re.match(r'^[^ ;]+:',line):block=line.split(':')[0]
  for slot in slots:
   if re.search(r'= load ptr, ptr '+re.escape(slot)+r'(?:[, ]|$)',line):loads.append({'slot':slot,'block':block,'instruction':line.strip()})
 assert len(loads)==len(slots),(arm,len(loads),len(slots))
 expected='arrlike.ic.shape.' if arm=='r6' else 'tav.get.slow.'
 assert all(row['block'].startswith(expected) for row in loads),(arm,loads)
 records.append({'arm':arm,'ir_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'loads':loads})
obj=w/'access-worker.o';cmd=['/opt/homebrew/opt/llvm/bin/llvm-objdump','--disassemble','--no-show-raw-insn','--reloc',str(obj)]
assembly=subprocess.check_output(cmd,text=True);(w/'access-machine.s').write_text(assembly)
(w/'cache-load-placement.json').write_text(json.dumps({'ir':records,'machine_command':cmd,'object_sha256':hashlib.sha256(obj.read_bytes()).hexdigest()},indent=2)+'\n')
print('IR cache loads all moved into shape tier; machine code written for inspection.')
