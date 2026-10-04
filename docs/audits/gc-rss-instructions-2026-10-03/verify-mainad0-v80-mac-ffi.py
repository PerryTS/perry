from pathlib import Path
import json,hashlib,re
B=Path('/Users/amlug/projects/perry/secret-tests/scratchpad/rss-header-20261002')
W=Path('/Users/amlug/projects/perry/rss-gc-runtime-mainad0-20261004')
V='mainad0-v80-mac-ffi'
def sha(p):
    with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
r=json.loads((B/(V+'-result.json')).read_text())
m=json.loads((B/'mainad0-once-v72-inputs.json').read_text())
assert len(m)==5014 and all(sha(W/n)==h for n,h in m.items())
assert r['rc']==0 and r['raw_log_sha256']==sha(B/(V+'.log'))
assert r['source_manifest_sha256']==sha(B/'mainad0-once-v72-inputs.json')
assert r['command']==['cargo','+nightly-2026-08-20','test','--locked','--release','-p','perry-ffi','-p','perry-ext-events']
text=(B/(V+'.log')).read_text()
assert 'Compiling perry-runtime ' in text
counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; 0 measured; 0 filtered out;',text)
assert counts==[('15','0','0'),('75','0','0'),('0','0','0'),('5','0','13')]
assert re.search(r'Running unittests .*perry_ext_events-',text)
assert re.search(r'Running unittests .*perry_ffi-',text)
assert 'Doc-tests perry_ext_events' in text and 'Doc-tests perry_ffi' in text
binary_paths=[Path(p) for p in re.findall(r'Running unittests .*?\(([^)]+)\)',text)]
assert len(binary_paths)==2 and all(p.is_file() for p in binary_paths)
p=dict(source_count=5014,result=r,suites=counts,test_binary_hashes={str(p):sha(p) for p in binary_paths},scope='Pinned mainad0 integration: Mac FFI/events/doc tests pass; source, native Cargo result, raw suites and test binaries independently verified.')
(B/(V+'-verification.json')).write_text(json.dumps(p,indent=2)+'\n')
print(json.dumps(p))
