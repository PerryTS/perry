from pathlib import Path
import hashlib, json, os, shlex, subprocess
root=Path(__file__).resolve().parents[4]
bench=root/'benchmarks/json_performance'
work=Path(__file__).resolve().parent
host='perry@perry-macos.local'
remote='/Users/perry/json-codex-yHdsko/benchmarks/json_performance'
provenance=json.loads((work/'provenance.json').read_text())
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==provenance['source_commit']
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
assert (work/'validation.json').exists()
assert all(x['same'] for x in json.loads((work/'root-fingerprint-comparison.json').read_text()))
assert next(x for x in json.loads((work/'current-proof.json').read_text()) if x['subject']=='short-native')['exit_code']==0
checks=json.loads((work/'codegen-comparison.json').read_text())['checks']
workers=[c for c in checks if c['mode']=='native' and c['scope']=='workers']
shadow=[c for c in checks if c['mode']=='shadow']
assert len(workers)==1 and workers[0]['exit_code']==0
assert len(shadow)==2 and all(c['exit_code']==0 for c in shadow)
token='json-r5-stage-'+str(os.getpid())
def ssh(code):
    return subprocess.run(['ssh',host,'python3 -c '+shlex.quote(code)],check=True)
ssh('from pathlib import Path; p=Path.home()/"bench.lock"; p.mkdir(); (p/"owner").write_text('+repr(token)+')')
try:
    files=['run_access.py','run_dispatch_focus.py','run_regression_focus.py','with_lock.py',
           'worker.ts','worker.js','access-worker.ts','access-worker.js']
    files+=['.work/invariant-field-loop-r5/'+n for n in ['worker','access-worker','rotating-worker',
            'provenance.json','source.patch','gc-witness.json','codegen-comparison.json',
            'candidate-short','main-short','r3-short','short-loops.ts','short-loops.js',
            'run-short-loops.py','short-provenance.json','short-reference-provenance.json','short-validation.json']]
    subprocess.run(['rsync','-aR']+files+[host+':'+remote+'/'],cwd=bench,check=True)
    expected={f:hashlib.sha256((bench/f).read_bytes()).hexdigest() for f in files}
    for name in ['worker','main-access']:
        path='.work/main-53df-fresh/'+name
        expected[path]=hashlib.sha256((bench/path).read_bytes()).hexdigest()
    ssh('from pathlib import Path; import hashlib; root=Path('+repr(remote)+'); expected='+repr(expected)+'; actual={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in expected}; assert actual==expected; print("VERIFIED",len(expected),"hashes")')
    (work/'remote-stage-hashes.json').write_text(json.dumps(expected,indent=2)+'\n')
finally:
    ssh('from pathlib import Path; p=Path.home()/"bench.lock"; assert (p/"owner").read_text()=='+repr(token)+'; (p/"owner").unlink(); p.rmdir()')
