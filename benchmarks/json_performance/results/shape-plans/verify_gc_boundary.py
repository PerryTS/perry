from pathlib import Path
import subprocess,json,hashlib
root=Path(__file__).resolve().parents[4];base='454daac4f8fc667ab4bc85b7b5b36c8bae56ae28'
subprocess.run(['git','diff','--exit-code',base,'--','crates/perry-runtime/src/gc',':(exclude)crates/perry-runtime/src/gc/tests',':(exclude)crates/perry-runtime/src/gc/verify.rs',':(exclude)crates/perry-runtime/src/gc/roots.rs'],cwd=root,check=True)
for name in ['roots.rs','verify.rs']:
 path='crates/perry-runtime/src/gc/'+name
 original=subprocess.check_output(['git','show',base+':'+path],cwd=root,text=True)
 actual=(root/path).read_text()
 if name=='roots.rs':
  actual=actual.replace('                    // Metadata keys must name the canonical owner even when\n                    // an ordinary value may retain a permanent growth alias.\n','')
 else:
  start=actual.index('#[cold]\npub(super) fn check_forwarded_reference(')
  end=actual.index('#[cold]\npub(super) fn panic_stale_forwarded_reference(',start)
  actual=actual[:start]+actual[end:]
 actual=actual.replace('check_forwarded_reference(', 'panic_stale_forwarded_reference(')
 assert actual==original, 'Unexpected non-diagnostic GC change in '+name
print('GC production differences are exclusively the diagnostic retained-growth check and its call sites; collector policy, allocation hooks, thresholds and parse-boundary hooks match base.')
