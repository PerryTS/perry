from pathlib import Path
import hashlib, json, subprocess

w = Path(__file__).resolve().parent
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
objects = []
for name in ['worker', 'access-worker', 'rotating-worker', 'options']:
    a, b = w / ('main-' + name + '.o'), w / ('candidate-' + name + '.o')
    assert a.read_bytes() == b.read_bytes(), name
    objects.append(dict(worker=name, same=True, sha256=sha(a)))
(w / 'worker-object-comparison.json').write_text(json.dumps(objects, indent=2) + '\n')
binary = w / 'candidate-rotating-worker'
symbols = subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm', '--defined-only', str(binary)], text=True)
selected = [line.split()[-1] for line in symbols.splitlines()
            if any(s in line for s in ['parse_string_value', 'string_from_json_bytes', 'string_from_json_source_bytes'])]
assert any('string_from_json_source_bytes' in s for s in selected)
records = []
for i, symbol in enumerate(selected):
    cmd = ['/opt/homebrew/opt/llvm/bin/llvm-objdump', '--disassemble-symbols=' + symbol,
           '--no-show-raw-insn', str(binary)]
    output = subprocess.check_output(cmd, text=True)
    file = w / ('candidate-parse-string-' + str(i) + '.s')
    file.write_text(output)
    records.append(dict(symbol=symbol, command=cmd, file=file.name, binary_sha256=sha(binary)))
(w / 'candidate-parser-machine.json').write_text(json.dumps(records, indent=2) + '\n')
print('All four worker objects match main; archived candidate string parse/allocation code.')
