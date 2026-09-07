from pathlib import Path
import subprocess,json,hashlib,re
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');out=art/'growth-alias-code';out.mkdir(exist_ok=True)
objdump=subprocess.check_output(['xcrun','--find','llvm-objdump'],text=True).strip()
for arm in ['short-array','growth-alias']:
 worker=art/(arm+'-worker');symbols=subprocess.check_output(['nm',str(worker)],text=True)
 wanted=['parse_api10parse_slow','parse_api17parse_result_slow','DirectParser11parse_array','DirectParser18parse_array_prefix','DirectParser16parse_array_tail','DirectParser18finish_short_array','mutation26resolve_materialized_array','growth_forwarding24is_retained_growth_alias','verify25check_forwarded_reference','js_array_grow']
 counts={}
 for part in wanted:
  matches=[line.split()[-1] for line in symbols.splitlines() if line.split()[-1].endswith(part) and line.split()[-2].lower()=='t']
  if not matches:
   counts[part]=None
   continue
  assert len(matches)==1,(part,matches)
  symbol=matches[0];name=re.sub(r'^.*?\d+', '',part)
  raw=subprocess.check_output([objdump,'--disassemble-symbols='+symbol,'--no-show-raw-insn',str(worker)],text=True)
  raw='\n'.join(line.expandtabs(4).rstrip() for line in raw.splitlines()).strip()+'\n'
  (out/(arm+'-'+name+'.asm')).write_text(raw)
  counts[symbol]=sum(bool(re.match(r'^\s*[0-9a-f]+:',line)) for line in raw.splitlines())
 record={'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'instructions_by_symbol':counts}
 (out/(arm+'.json')).write_text(json.dumps(record,indent=2)+'\n');print(arm,counts,flush=True)
