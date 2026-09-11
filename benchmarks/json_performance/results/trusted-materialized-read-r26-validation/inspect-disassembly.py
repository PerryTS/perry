from pathlib import Path
import hashlib,json,subprocess
w=Path(__file__).resolve().parent;records=[]
for arm in ['main','candidate']:
 binary=w/(arm+'-access-worker');symbols=subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm','--defined-only',str(binary)],text=True)
 wanted=['_js_array_get_f64','_js_packed_arraylike_index_get']+[line.split()[-1] for line in symbols.splitlines() if line.split() and any(line.split()[-1].endswith(s) for s in ['8lazy_get','15lazy_get_rooted','26resolve_materialized_array'])]
 functions=[]
 for symbol in wanted:
  label='array-get' if symbol=='_js_array_get_f64' else 'packed-index-get' if symbol=='_js_packed_arraylike_index_get' else symbol.split('json_tape')[-1]
  dest=w/(arm+'-'+label+'.s');cmd=['/opt/homebrew/opt/llvm/bin/llvm-objdump','--disassemble-symbols='+symbol,'--no-show-raw-insn',str(binary)];raw=subprocess.check_output(cmd);assert len(raw)>100;dest.write_bytes(raw);functions.append({'symbol':symbol,'command':cmd,'output':dest.name,'sha256':hashlib.sha256(raw).hexdigest()})
 records.append({'arm':arm,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'functions':functions})
for name in ['worker','access-worker','rotating-worker','options']:assert (w/('main-'+name+'.o')).read_bytes()==(w/('candidate-'+name+'.o')).read_bytes(),name
(w/'read-disassembly.json').write_text(json.dumps({'arms':records,'all_four_worker_objects_identical':True},indent=2)+'\n')
print('Recorded linked disassembly and four object equivalences')
