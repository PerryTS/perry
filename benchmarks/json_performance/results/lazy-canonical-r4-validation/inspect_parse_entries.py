from pathlib import Path
import json, re, subprocess, sys
root=Path(__file__).resolve().parent
out={}
for label in [sys.argv[2] if len(sys.argv) > 2 else 'decoder-r2', sys.argv[1]]:
    worker=root/label/'worker'
    raw=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm','--demangle','--numeric-sort',str(worker)],text=True)
    syms=[]
    for line in raw.splitlines():
        m=re.match(r'([0-9a-f]+) ([Tt]) (.+)',line)
        if m:syms.append((int(m[1],16),m[3]))
    out[label]={}
    for i,(addr,name) in enumerate(syms):
        if name == '_js_json_parse' or any(s in name for s in ['::nesting_depth_exceeds','::contains_open_container_tail','::parse_noncontainer','::parse_slow','::allocate_empty_object']):
            end=next((a for a,n in syms[i+1:] if a>addr),addr)
            asm=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-objdump','--demangle','--disassemble',f'--start-address={addr}',f'--stop-address={end}',str(worker)],text=True)
            out[label][name]={'start':hex(addr),'bytes':end-addr,'assembly':asm}
            print(label,name,end-addr,'bytes',hex(addr))
(root/sys.argv[1]/'entry-codegen.json').write_text(json.dumps(out,indent=2)+'\n')
