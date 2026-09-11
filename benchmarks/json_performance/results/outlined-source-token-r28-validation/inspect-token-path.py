from pathlib import Path
import hashlib,json,re,subprocess
w=Path(__file__).resolve().parent;records=[]
names=['string_from_dominant_token','source_token_utf16_len','parse_string_value','string_from_json_bytes']
for arm in ['main','candidate']:
 binary=w/(arm+'-worker');symbols=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm','--defined-only',str(binary)],text=True)
 wanted=[line.split()[-1] for line in symbols.splitlines() if line.split() and any(n in line.split()[-1] for n in names)]
 for n in names[:2]:assert any(n in s for s in wanted)==(arm=='candidate'),(arm,n,wanted)
 for symbol in wanted:
  kind=next(n for n in names if n in symbol);label=kind+'-'+hashlib.sha256(symbol.encode()).hexdigest()[:8];dest=w/(arm+'-'+label+'.s')
  cmd=['/opt/homebrew/opt/llvm/bin/llvm-objdump','--disassemble-symbols='+symbol,'--no-show-raw-insn',str(binary)];raw=subprocess.check_output(cmd);dest.write_bytes(raw)
  instructions=[m[1] for line in raw.decode().splitlines() if (m:=re.match(r'^[0-9a-f]+:\s+(.*)',line))]
  r=dict(arm=arm,kind=kind,symbol=symbol,command=cmd,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),output=dest.name,output_sha256=hashlib.sha256(raw).hexdigest(),instructions=len(instructions),calls=[x for x in instructions if re.match(r'(?:bl|blr)\s',x)])
  records.append(r);print(arm,kind,r['instructions'],flush=True)
(w/'token-path-disassembly.json').write_text(json.dumps(dict(note='Static linked function sizes and call targets only; not dynamic attribution or semantic proof.',records=records),indent=2)+'\n')
for r in records:assert hashlib.sha256((w/r['output']).read_bytes()).hexdigest()==r['output_sha256']
