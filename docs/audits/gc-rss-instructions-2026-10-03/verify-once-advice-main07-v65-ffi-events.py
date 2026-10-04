"""Bind current-main FFI/event test results to full runtime source inputs."""
import pathlib, json, hashlib, re, tarfile
B = pathlib.Path('/root/rss-header-20261002')
R = B / 'primary-once-advice-main07-v65-ffi-events'
E = R / 'export'

def sha(p):
    with p.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

assert (B / 'gc-once-advice-main07-v65-ffi-events-driver.exit').read_text().strip() == '0'
assert (E / 'complete.exit').read_text().strip() == '0'
result = json.loads((E / 'result.json').read_text())
inputs = json.loads((E / 'source-inputs.json').read_text())
control = json.loads((B / 'primary-entry-once-main07-v45/export/source-inputs.json').read_text())
assert inputs == control and len(inputs) == 5011
assert all(sha(R / 'source' / n) == h for n, h in inputs.items())
assert result['base'] == '07e50b14a3d23e8f914193ec4d5a679adbd7c37a'
assert result['production_runtime'] == '4401b3b981'
assert result['rc'] == 0
assert result['command'] == ['cargo', 'test', '--locked', '--release', '-p', 'perry-ffi', '-p', 'perry-ext-events']
assert sha(E / 'test.log') == result['log_sha256']
assert sha(E / 'source-inputs.json') == result['source_inputs_sha256']
text = (E / 'test.log').read_text()
assert 'Compiling perry-runtime ' in text
counts = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; 0 measured; 0 filtered out;', text)
assert counts == [('15', '0', '0'), ('75', '0', '0'), ('0', '0', '0'), ('5', '0', '13')], counts
proof = dict(base=result['base'], production_runtime=result['production_runtime'], source_count=5011, suites=counts, result=result,
             scope='Current07 Linux FFI/event and doc tests; complete raw log, source hashes and command independently verified.')
(E / 'independent-verification.json').write_text(json.dumps(proof, indent=2) + '\n')
files = [p for p in E.iterdir() if p.is_file()] + [pathlib.Path(__file__), B / 'primary-once-advice-main07-v65-ffi-events.py', B / 'gc-once-advice-main07-v65-ffi-events-driver.exit']
manifest = B / 'once-advice-main07-v65-ffi-events-evidence-files.json'
manifest.write_text(json.dumps({str(p.relative_to(B)): sha(p) for p in files}, indent=2) + '\n')
with tarfile.open(B / 'once-advice-main07-v65-ffi-events-evidence.tar.gz', 'w:gz') as t:
    for p in files + [manifest]:
        t.add(p, arcname=str(p.relative_to(B)))
print(json.dumps(proof))
