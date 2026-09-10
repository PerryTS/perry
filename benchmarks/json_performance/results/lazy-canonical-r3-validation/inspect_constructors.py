from pathlib import Path
import json,subprocess,re,sys
base=Path('benchmarks/json_performance/.work')
res={}
for label in [sys.argv[2] if len(sys.argv) > 2 else 'decoder-r2',sys.argv[1]]:
    p=base/label/'rotating-worker'
    nm=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm','--demangle','--numeric-sort',str(p)],text=True)
    syms=[]
    for line in nm.splitlines():
        m=re.match(r'([0-9a-f]+) ([Tt]) (.+)',line)
        if m: syms.append((int(m[1],16),m[3]))
    matches={}
    for i,(addr,name) in enumerate(syms):
        if any(s in name for s in ['::parse_string_value','::alloc_large_borrowed_string','::string_from_json_bytes','::borrowed_source_utf16_len']):
            end=next((a for a,n in syms[i+1:] if a>addr),addr)
            asm=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-objdump','--demangle','--disassemble',f'--start-address={addr}',f'--stop-address={min(end,addr+72)}',str(p)],text=True)
            matches[name]={'start':hex(addr),'bytes':end-addr,'entry':asm}
    res[label]=matches
(base/sys.argv[1]/'codegen-comparison.json').write_text(json.dumps(res,indent=2)+'\n')
for label,matches in res.items():
    for name,r in matches.items(): print(label,name,r['bytes'],r['entry'].splitlines()[6:9])
