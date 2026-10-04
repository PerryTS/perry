"""Bind current-main FFI/event test results to full runtime source inputs."""
import pathlib, json, hashlib, re, tarfile
B = pathlib.Path('/root/rss-header-20261002')
R = B / 'primary-mainad0-v80-ffi-events'
E = R / 'export'

def sha(p):
    with p.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

assert (B / 'gc-mainad0-v80-ffi-events-driver.exit').read_text().strip() == '0'
assert (E / 'complete.exit').read_text().strip() == '0'
result = json.loads((E / 'result.json').read_text())
inputs = json.loads((E / 'source-inputs.json').read_text())
control = json.loads((B / 'primary-mainad0-once-v74/export/source-inputs.json').read_text())
assert inputs == control and len(inputs) == 5014
assert all(sha(R / 'source' / n) == h for n, h in inputs.items())
assert result['base'] == 'ad0a2617bf0b9708032cf89c86575e9bd69fb074'
assert result['production_runtime'] == '0aa483aa91ab901583002ec0174a50884b60ad81'
assert result['rc'] == 0
assert result['command'] == ['cargo', 'test', '--locked', '--release', '-p', 'perry-ffi', '-p', 'perry-ext-events']
assert sha(E / 'test.log') == result['log_sha256']
assert sha(E / 'source-inputs.json') == result['source_inputs_sha256']
text = (E / 'test.log').read_text()
assert 'Compiling perry-runtime ' in text
counts = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; 0 measured; 0 filtered out;', text)
assert counts == [('15', '0', '0'), ('75', '0', '0'), ('0', '0', '0'), ('5', '0', '13')], counts
binary_paths = [pathlib.Path(p) for p in re.findall(r'Running unittests .*?\(([^)]+)\)', text)]
assert len(binary_paths) == 2 and all(p.is_file() for p in binary_paths)
test_binary_hashes = {str(p): sha(p) for p in binary_paths}
proof = dict(test_binary_hashes=test_binary_hashes, base=result['base'], production_runtime=result['production_runtime'], source_count=5014, suites=counts, result=result,
             scope='Pinned mainad0 integration Linux FFI/event and doc tests; complete raw log, source hashes and command independently verified.')
(E / 'independent-verification.json').write_text(json.dumps(proof, indent=2) + '\n')
files = [p for p in E.iterdir() if p.is_file()] + [pathlib.Path(__file__), B / 'primary-mainad0-v80-ffi-events.py', B / 'gc-mainad0-v80-ffi-events-driver.exit']
manifest = B / 'mainad0-v80-ffi-events-evidence-files.json'
manifest.write_text(json.dumps({str(p.relative_to(B)): sha(p) for p in files}, indent=2) + '\n')
with tarfile.open(B / 'mainad0-v80-ffi-events-evidence.tar.gz', 'w:gz') as t:
    for p in files + [manifest]:
        t.add(p, arcname=str(p.relative_to(B)))
print(json.dumps(proof))
