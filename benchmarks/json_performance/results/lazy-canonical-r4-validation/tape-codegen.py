from pathlib import Path
import json,re,subprocess,hashlib
base=Path('benchmarks/json_performance/.work')
result={}
for label in ['main-eee','tape-depth-r2','lazy-canonical-r3','lazy-canonical-r4']:
    worker=base/label/'worker'
    raw=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm','--demangle','--numeric-sort',str(worker)],text=True)
    syms=[]
    for line in raw.splitlines():
        m=re.match(r'([0-9a-f]+) ([Tt]) (.+)',line)
        if m: syms.append((int(m[1],16),m[3]))
    functions=[]
    for i,(addr,name) in enumerate(syms):
        if any(s in name for s in ['::build_tape_into','::parse_slow','::try_parse_via_tape','::parse_deep_or_throw','::try_parse_deep_iterative']):
            end=next((a for a,n in syms[i+1:] if a>addr),addr)
            asm=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-objdump','--demangle','--disassemble',f'--start-address={addr}',f'--stop-address={end}',str(worker)],text=True)
            functions.append({'name':name,'start':hex(addr),'span_bytes':end-addr,'assembly':asm})
            print(label,name,hex(addr),end-addr,flush=True)
    result[label]={'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'functions':functions}
(base/'lazy-canonical-r4/tape-codegen.json').write_text(json.dumps(result,indent=2)+'\n')
