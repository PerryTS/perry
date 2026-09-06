from pathlib import Path
import subprocess,json,hashlib,re
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');out=art/'plan-scan-code';out.mkdir(exist_ok=True)
objdump=subprocess.check_output(['xcrun','--find','llvm-objdump'],text=True).strip()
for arm in ['escaped-count','plan-scan']:
 worker=art/(arm+'-worker');symbols=subprocess.check_output(['nm',str(worker)],text=True)
 wanted=['14stringify_flat10emit_piece','14stringify_flat12string_piece','14stringify_flat11emit_object','23stringify_record_output11emit_record']
 counts={}
 for part in wanted:
  matches=[line.split()[-1] for line in symbols.splitlines() if part in line and line.split()[-2].lower()=='t']
  assert len(matches)==1,(part,matches)
  symbol=matches[0];name=part.split('stringify_')[-1];name=re.sub(r'^.*?\d+', '',name)
  raw=subprocess.check_output([objdump,'--disassemble-symbols='+symbol,'--no-show-raw-insn',str(worker)],text=True)
  raw='\n'.join(line.expandtabs(4).rstrip() for line in raw.splitlines()).strip()+'\n'
  (out/(arm+'-'+name+'.asm')).write_text(raw)
  counts[symbol]=sum(bool(re.match(r'^\s*[0-9a-f]+:',line)) for line in raw.splitlines())
 record={'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'instructions_by_symbol':counts}
 (out/(arm+'.json')).write_text(json.dumps(record,indent=2)+'\n');print(arm,counts,flush=True)
