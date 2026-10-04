import pathlib,json,hashlib,re,subprocess
B=pathlib.Path(__file__).resolve().parent
W=pathlib.Path('/Users/amlug/projects/perry/rss-gc-mark-frontier-mainad0-20261004')
V='mark-frontier-mainad0-v81'; C='2c3b34ebd5d0ae63a10ae2b6f9360026fc2a0d20'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
m=json.loads((B/(V+'-inputs.json')).read_text());r=json.loads((B/(V+'-mac-result.json')).read_text());assert len(m)==5015
# Read pinned Git blobs, so later private revisions cannot silently change evidence.
requests=''.join(C+':'+n+'\n' for n in m).encode()
data=subprocess.run(['git','cat-file','--batch'],cwd=W,input=requests,stdout=subprocess.PIPE,check=True).stdout
cursor=0
for n,h in m.items():
 end=data.index(b'\n',cursor);header=data[cursor:end].split();assert len(header)==3 and header[1]==b'blob',n
 size=int(header[2]);cursor=end+1;assert hashlib.sha256(data[cursor:cursor+size]).hexdigest()==h,n;cursor+=size;assert data[cursor:cursor+1]==b'\n';cursor+=1
assert cursor==len(data)
assert r['rc']==0 and r['source_manifest_sha256']==sha(B/(V+'-inputs.json')) and r['raw_log_sha256']==sha(B/(V+'-mac-runtime.log'))
log=(B/(V+'-mac-runtime.log')).read_text();assert re.search(r'test result: ok\. 4883 passed; 0 failed; 5 ignored; 0 measured; 0 filtered out;',log)
for n in ['cyclic_chain_keeps_only_pending_headers_across_budgeted_steps','shared_binary_tree_preserves_all_live_objects_with_a_small_frontier']:assert 'test gc::tests::mark_frontier::'+n+' ... ok' in log,n
bins=[pathlib.Path(p) for p in re.findall(r'Running unittests .*?\(([^)]+)\)',log)];assert len(bins)==1
proof=dict(base='ad0a2617bf0b9708032cf89c86575e9bd69fb074',private_commit=C,source_count=len(m),runtime=dict(passed=4883,failed=0,ignored=5),result=r,test_binary_hashes={str(p):sha(p) for p in bins},subjects=['4096-object cycle: all reachable marked, seven-header step budget, frontier capacity <=8 pointers','8191-object binary tree with root aliases: all reachable marked, seven-header step budget, frontier capacity <=32 pointers'],scope='Private mark-only pending-frontier experiment, Mac full runtime. No application RSS/instruction result yet; not adopted.')
(B/(V+'-mac-verification.json')).write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof))
