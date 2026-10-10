from pathlib import Path
import json,re,hashlib
L=Path('/root/lanes/perry-unpackcpu'); V=L/'validation'; R=L/'perry'
records=[]
for name in ['stdlib-filtered','test_gap_crypto_wide','test_gap_crypto_byte_update_encoding','test_gap_crypto_hash_chain_11516','test_gap_zlib_cpu_inflate','test_gap_native_payload_zlib_one_shots','test_gap_zlib_chunk_backpressure','node-version','file-size','diff']:
    rc=int((V/(name+'.rc')).read_text())
    assert rc==(1 if name=='test_gap_native_payload_zlib_one_shots' else 0),(name,rc)
    if rc:
        gap=json.loads((L/'evidence/inherited-compression-gap.json').read_text())
        assert gap['round_trip_and_remaining_output_parity']
    log=(V/(name+'.log')).read_text(errors='replace')
    text=re.sub(r'\x1b\[[0-9;]*m','',log)
    summaries=[line.strip() for line in text.splitlines() if any(term in line.lower() for term in ['test result:', 'passed:', 'failed:', 'total:', 'tests passed', 'tests failed', 'all tests','parity tests','parity pass:','parity fail:','compile fail:','crashed:'])]
    records.append(dict(name=name,rc=rc,summary=summaries[-12:],inherited_encoder_gap=(name=='test_gap_native_payload_zlib_one_shots'),log_sha256=hashlib.sha256(log.encode()).hexdigest()))
log=(L/'inflate-unit-final.log').read_text()
assert (L/'inflate-unit-final.rc').read_text().strip()=='0'
records.append(dict(name='cargo test --release -p perry-ext-zlib -- --test-threads=1',rc=0,summary=[line for line in log.splitlines() if 'test result:' in line]))
full=(L/'stdlib-combined-full.log').read_text()
failed=[line.strip() for line in full.splitlines() if line.startswith('    runtime_thread_exit_tests::')]
base_failed=[line.strip() for line in (L/'stdlib-base-tests.log').read_text().splitlines() if line.startswith('    runtime_thread_exit_tests::')]
assert sorted(failed)==sorted(base_failed) and len(failed)==2
records.append(dict(name='cargo test --release -p perry-stdlib -- --test-threads=1',rc=int((L/'stdlib-combined-full.rc').read_text()),inherited_failures=failed,summary=[line for line in full.splitlines() if 'test result:' in line]))
negative=[]
for stage,arms in [('hash',['base','fix']),('inflate',['sha','fix'])]:
    for arm in arms:
        for i in range(5):
            stem=f'{stage}-budget-{arm}-{i}'
            row=json.loads((V/(stem+'.json')).read_text())
            assert row['parity'] and row['pass_budget']==(arm=='fix')
            negative.append(dict(control=stem,**row))
row=json.loads((V/'hash-budget-fix-masked.json').read_text())
assert row['parity'] and not row['pass_budget']
negative.append(dict(control='hash-budget-fix-masked',**row))
(L/'evidence/tests.json').write_text(json.dumps(records,indent=2)+'\n')
(L/'evidence/negative-controls.json').write_text(json.dumps(negative,indent=2)+'\n')
print(json.dumps(records,indent=2))
print('All 21 expected backend controls have matching Node output and expected budget outcomes.')
