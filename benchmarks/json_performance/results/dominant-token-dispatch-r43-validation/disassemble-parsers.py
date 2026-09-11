from pathlib import Path
import hashlib,json,re,subprocess
w=Path(__file__).resolve().parent
rows=[]
for arm in ['main','candidate']:
 binary=w/(arm+'-rotating-worker')
 for function in ['18parse_string_value','20parse_object_untyped','23parse_string_bytes_slow']:
  symbol='__RNvMs0_NtNtCs3gRpqoBkMHm_13perry_runtime4json6parserNtB5_12DirectParser'+function
  cmd=['/opt/homebrew/opt/llvm/bin/llvm-objdump','--disassemble-symbols='+symbol,'--no-show-raw-insn',str(binary)]
  data=subprocess.check_output(cmd);target=w/(arm+'-'+function+'.s');target.write_bytes(data)
  ins=[line for line in data.decode().splitlines() if re.match(r'\s*[0-9a-f]+:',line)]
  assert ins
  rows.append(dict(arm=arm,function=function,instructions=len(ins),prologue=ins[:16],command=cmd,sha256=hashlib.sha256(data).hexdigest(),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest()))
  print(arm,function,len(ins))
(w/'parser-disassembly.json').write_text(json.dumps(dict(rows=rows,note='Linked production instruction counts and prologues; diagnostic, not timing or causal proof.'),indent=2)+'\n')
