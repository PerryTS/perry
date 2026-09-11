from pathlib import Path
import hashlib, json, os, subprocess

w = Path(__file__).resolve().parent
root = w.parents[3]
head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=root)
paths = ['crates/perry-runtime/src/json/replacer.rs', 'crates/perry-runtime/src/string/concat.rs', 'crates/perry-runtime/src/string/append.rs', 'crates/perry-runtime/src/string/mod.rs', 'crates/perry-runtime/src/string/json_flags_tests.rs', 'crates/perry-runtime/src/string/json_construction.rs', 'crates/perry-runtime/src/string/utf16_count.rs', 'test-files/test_json_string_emitters.ts']
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
subprocess.run(['python3', str(w / 'build-release.py')], cwd=root, check=True)
