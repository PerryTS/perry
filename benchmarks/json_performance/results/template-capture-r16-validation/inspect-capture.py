from pathlib import Path
import hashlib,json,re,subprocess,sys
w=Path(__file__).resolve().parent;arm='main' if '--main' in sys.argv else 'candidate';binary=w/(arm+'-rotating-worker');llvm=Path('/opt/homebrew/opt/llvm/bin')
symbols=subprocess.check_output([str(llvm/'llvm-nm'),'--defined-only',str(binary)],text=True)
selected=[line.split()[-1] for line in symbols.splitlines() if 'remember_parse_object_template' in line]
assert len(selected)==1
symbol=selected[0];cmd=[str(llvm/'llvm-objdump'),'--disassemble-symbols='+symbol,'--no-show-raw-insn',str(binary)];code=subprocess.check_output(cmd,text=True);dest=w/(arm+'-template-capture.s');dest.write_text(code)
indirect_cmd=[str(llvm/'llvm-objdump'),'--macho','--indirect-symbols',str(binary)];indirect=subprocess.check_output(indirect_cmd,text=True);(w/(arm+'-capture-indirect-symbols.txt')).write_text(indirect)
stubs={int(a,16):n for a,n in re.findall(r'^(0x[0-9a-f]+)\s+\d+\s+(\S+)$',indirect,re.M)}
instructions=[x for x in code.splitlines() if re.match(r'\s*[0-9a-f]+:',x)];prologue=instructions[:12]
frame=sum(int(m[1],16) for x in prologue if (m:=re.search(r'sub\s+sp, sp, #0x([0-9a-f]+)',x)))+sum(int(m[1],16) for x in prologue if (m:=re.search(r'\[sp, #-0x([0-9a-f]+)\]!',x)))
calls=[]
for i,line in enumerate(instructions):
 m=re.search(r'bl\s+(0x[0-9a-f]+)',line)
 if m and int(m[1],16) in stubs:
  calls.append({'instruction':line,'symbol':stubs[int(m[1],16)],'context':instructions[max(0,i-6):i+1]})
record={'arm':arm,'symbol':symbol,'command':cmd,'indirect_command':indirect_cmd,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'assembly_file':dest.name,'assembly_sha256':hashlib.sha256(code.encode()).hexdigest(),'prologue':prologue,'stack_bytes':frame,'external_calls':calls}
(w/(arm+'-template-capture-machine.json')).write_text(json.dumps(record,indent=2)+'\n')
print(arm,'capture frame',frame,'memcpy calls',sum(x['symbol']=='_memcpy' for x in calls),flush=True)
if arm=='candidate':
 main=json.loads((w/'main-template-capture-machine.json').read_text())
 assert sum(x['symbol']=='_memcpy' for x in main['external_calls'])==2
 assert sum(x['symbol']=='_memcpy' for x in calls)==1
 assert frame<main['stack_bytes']
 (w/'capture-machine-comparison.json').write_text(json.dumps({'main':main,'candidate':record,'verdict':'One intermediate memcpy removed; one final cache-publication memcpy retained; candidate frame smaller.'},indent=2)+'\n')
