from pathlib import Path
import json,hashlib,re
B=Path('/Users/amlug/projects/perry/secret-tests/scratchpad/rss-header-20261002')
W=Path('/Users/amlug/projects/perry/rss-gc-runtime-latest2026-20261003')
V='once-advice-main07-v65-mac-ffi'
def sha(p):
    with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
r=json.loads((B/(V+'-result.json')).read_text())
m=json.loads((B/'entry-once-main07-v45-inputs.json').read_text())
assert len(m)==5011 and all(sha(W/n)==h for n,h in m.items())
assert r['rc']==0 and r['raw_log_sha256']==sha(B/(V+'.log'))
assert r['source_manifest_sha256']==sha(B/'entry-once-main07-v45-inputs.json')
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
p=dict(source_count=5011,result=r,suites=counts,test_binary_hashes={str(p):sha(p) for p in binary_paths},scope='Exact adopted once-advice candidate: Mac FFI/event/doc tests pass. Original driver failed only its post-test suite-order assertion; native Cargo rc=0. Original raw log/result preserved, no native rerun.')
(B/(V+'-verification.json')).write_text(json.dumps(p,indent=2)+'\n')
print(json.dumps(p))
