from pathlib import Path
import hashlib,json,re,subprocess,sys
base=Path(__file__).resolve().parent
candidate=sys.argv[1]
result={}
for label in [sys.argv[2] if len(sys.argv)>2 else 'tape-depth-r2',candidate]:
    worker=base/label/'worker'
    lines=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm','--demangle','--numeric-sort',str(worker)],text=True).splitlines()
    symbols=[]
    for line in lines:
        m=re.match(r'([0-9a-f]+) ([Tt]) (.+)',line)
        if m:symbols.append((int(m[1],16),m[3]))
    functions={}
    for i,(at,name) in enumerate(symbols):
        if name=='_js_json_stringify' or any(s in name for s in ['::try_stringify_lazy_array','::normalize_lazy_json_numbers','::stringify_lazy::','::stringify_value','::stringify_object_inner']):
            end=next((a for a,n in symbols[i+1:] if a>at),at)
            assembly=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-objdump','--demangle','--disassemble',f'--start-address={at}',f'--stop-address={end}',str(worker)],text=True)
            functions[name]={'start':hex(at),'symbol_span_bytes':end-at,'assembly':assembly}
            print(label,name,end-at)
    result[label]={'worker_sha256':hashlib.sha256(worker.read_bytes()).hexdigest(),'functions':functions}
(Path(sys.argv[3]) if len(sys.argv)>3 else base/candidate/'lazy-codegen.json').write_text(json.dumps(result,indent=2)+'\n')
