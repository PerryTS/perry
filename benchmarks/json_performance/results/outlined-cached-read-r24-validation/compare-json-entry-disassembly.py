from pathlib import Path
import hashlib,json,re,subprocess
w=Path(__file__).resolve().parent;old=w.with_name('materialized-read-r23')
inputs={'main':w/'main-worker','r22':old/'r22-shared-worker','r23':old/'candidate-worker'}
if (w/'candidate-worker').exists():inputs['r24']=w/'candidate-worker'
symbols=['_js_json_parse','_js_json_stringify','_js_json_stringify_full','_js_array_get_f64']
records=[]
for arm,binary in inputs.items():
 for symbol in symbols:
  command=['/opt/homebrew/opt/llvm/bin/llvm-objdump','--disassemble-symbols='+symbol,'--no-show-raw-insn',str(binary)]
  text=subprocess.check_output(command,text=True);dest=w/(arm+'-entry-'+symbol+'.s');dest.write_text(text)
  instructions=[];mnemonics=[]
  for line in text.splitlines():
   m=re.match(r'^[0-9a-f]+:\s+(.*)',line)
   if not m:continue
   instruction=m.group(1);mnemonics.append(instruction.split()[0]);instructions.append(instruction)
  records.append({'arm':arm,'symbol':symbol,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'disassembly':dest.name,'disassembly_sha256':hashlib.sha256(text.encode()).hexdigest(),'instructions':len(instructions),'mnemonics':mnemonics,'calls':[i for i in instructions if re.match(r'(?:bl|blr)\s',i)]})
(w/'json-entry-disassembly.json').write_text(json.dumps({'note':'Static entry-point instruction counts and call targets only; not a dynamic profile, semantic equivalence proof or attribution of timing regressions. Full original linked disassembly preserved.','records':records},indent=2)+'\n')
for r in records:print(r['arm'],r['symbol'],r['instructions'])
