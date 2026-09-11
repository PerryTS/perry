from pathlib import Path
import hashlib, json, os, subprocess

w = Path(__file__).resolve().parent
root = w.parents[3]
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=root)
paths = ['crates/perry-runtime/src/value/dynamic_arith.rs', 'test-files/test_json_index_remainder.ts']
hashes = {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in paths}
command = ['cargo', 'test', '--release', '-p', 'perry-runtime', '--lib', 'json']
record = {'source_commit': head, 'command': command, 'env': {'RUST_TEST_THREADS': '1'}, 'hashes': hashes}
(w / 'unit-source.json').write_text(json.dumps(record, indent=2) + '\n')
env = {k: v for k, v in os.environ.items() if not k.startswith('PERRY_')}
with (w / 'unit.log').open('wb') as log:
    result = subprocess.run(command, cwd=root, env=env | {'RUST_TEST_THREADS': '1'}, stdout=log, stderr=subprocess.STDOUT)
record['exit_code'] = result.returncode
(w / 'unit-source.json').write_text(json.dumps(record, indent=2) + '\n')
assert result.returncode == 0, 'Unit tests failed; do not start the production build.'
assert all(hashlib.sha256((root / p).read_bytes()).hexdigest() == h for p, h in hashes.items())
print((w / 'unit.log').read_text().split('test result:')[-1].strip(), flush=True)
# Run actual runtime arithmetic cases from the exact freshly built unit binary.
import re
matches = re.findall(r'Running unittests .*?\((.*?)\)', (w / 'unit.log').read_text())
assert len(matches) == 1
binary = (root / matches[0]).resolve()
listing = subprocess.check_output([str(binary), '--list'], text=True)
assert 'value::dynamic_arith::tests::dynamic_mod_preserves_u32_edges_and_exceptional_values: test' in listing
arithmetic_command = [str(binary), 'value::dynamic_arith::tests', '--test-threads=1']
with (w / 'arithmetic-suite.log').open('wb') as log:
    arithmetic = subprocess.run(arithmetic_command, env=env | {'RUST_TEST_THREADS': '1'}, stdout=log, stderr=subprocess.STDOUT)
(w / 'arithmetic-suite-provenance.json').write_text(json.dumps(dict(source_commit=head, command=arithmetic_command, env={'RUST_TEST_THREADS': '1'}, exit_code=arithmetic.returncode, binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest()), indent=2) + '\n')
assert arithmetic.returncode == 0
print((w / 'arithmetic-suite.log').read_text().split('test result:')[-1].strip(), flush=True)
subprocess.run(['python3', str(w / 'build-release.py')], cwd=root, check=True)
