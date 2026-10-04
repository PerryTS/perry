import pathlib,json,hashlib,re
B=pathlib.Path('/root/rss-header-20261002');P=B/'primary-idle-eden-v30/export';R=B/'primary-idle-eden-v30-residency';E=R/'export-r1'
def sha(p):
 with pathlib.Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
result=json.loads((E/'result.json').read_text());inputs=json.loads((E/'source-inputs.json').read_text());previous=json.loads((P/'source-inputs.json').read_text())
assert result['rc']==0 and (E/'complete.exit').read_text().strip()=='0'
assert sha(E/'test.log')==result['log_sha256'] and sha(E/'source-inputs.json')==result['source_inputs_sha256']
assert inputs.keys()==previous.keys() and len(inputs)==4994
assert all(sha(R/'source'/n)==h for n,h in inputs.items())
delta=[n for n in inputs if inputs[n]!=previous[n]]
assert delta==['crates/perry-runtime/src/arena/block/reuse_window_tests.rs'],delta
log=(E/'test.log').read_text();name='idle_eden_pages_leave_rss_without_releasing_recent_or_reused_blocks'
assert name in result['command'] and re.search(r'test .*'+name+r' \.\.\. ok',log)
assert re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)==[('1','0','0')]
assert (B/'gc-private-idle-eden-v30-residency-driver.exit').read_text().strip()=='1'
assert not (R/'export/result.json').exists()
proof=dict(tracked_inputs=len(inputs),test_only_delta=delta,result=result,independently_verified=True,original_setup_failure_preserved=True,scope='Private Linux residency/reuse test; moving-only page policy remains unapplied. Does not prove application peak savings.')
(E/'independent-verification.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof,indent=2))
